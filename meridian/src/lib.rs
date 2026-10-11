//! Shares `MAVLink` telemetry sessions over HTTP.
//!
//! A session is an append-only telemetry log in the `.tlog` layout of `nadir_core::tlog`, posted in
//! batches by one publisher and streamed to any number of readers. Records are never decoded here.

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse as _, Response};
use axum::routing::get;
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio_stream::StreamExt as _;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

mod session;

use session::{AppendError, Sessions, valid_id};

/// Comfortably above the publisher's batch size in `nadir_core::publish`.
const MAX_BATCH: usize = 4 * 1024 * 1024;

/// Serves the sessions stored in `dir`, creating it if needed.
pub async fn serve(listener: TcpListener, dir: PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(&dir)?;

    let app = Router::new()
        .route("/sessions", get(list))
        .route("/sessions/{id}", get(view).post(append))
        .layer(DefaultBodyLimit::max(MAX_BATCH))
        .with_state(Arc::new(Sessions::new(dir)));

    axum::serve(listener, app).await
}

async fn list(State(sessions): State<Arc<Sessions>>) -> Response {
    match sessions.summaries() {
        Ok(summaries) => summaries
            .into_iter()
            .map(|(id, bytes, idle)| format!("{id}\t{bytes}\t{}\n", idle.as_secs()))
            .collect::<String>()
            .into_response(),
        Err(e) => internal(&e),
    }
}

#[derive(Deserialize)]
struct ViewQuery {
    #[serde(default)]
    offset: u64,
    epoch: Option<u64>,
}

/// The session's history from `offset`, then whatever is appended while the reader stays attached.
///
/// Read from the start instead if `epoch` names an earlier session under this id or `offset` is past
/// the end. `x-session-offset` says where the body starts, `x-session-epoch` which session it is.
async fn view(
    State(sessions): State<Arc<Sessions>>,
    Path(id): Path<String>,
    Query(query): Query<ViewQuery>,
) -> Response {
    if !valid_id(&id) {
        return (StatusCode::BAD_REQUEST, "not a session id\n").into_response();
    }

    let session = match sessions.get(&id) {
        Ok(Some(session)) => session,
        Ok(None) => return (StatusCode::NOT_FOUND, "no such session\n").into_response(),
        Err(e) => return internal(&e),
    };

    let (mut start, mut history, mut tail, epoch) = session.attach(query.offset);
    if query.epoch.is_some_and(|expected| expected != epoch) {
        (start, history, tail, _) = session.attach(0);
    }
    tracing::info!("reader attached to {id} at byte {start}");

    let live = BroadcastStream::new(tail).map_while(move |batch| match batch {
        Ok(batch) => Some(Ok::<Bytes, Infallible>(batch)),
        // Skipping ahead would splice a gap into the stream; ending it lets the reader resume.
        Err(BroadcastStreamRecvError::Lagged(missed)) => {
            tracing::warn!("reader of {id} fell {missed} batches behind, ending its stream");
            None
        }
    });

    let body = Body::from_stream(tokio_stream::once(Ok(Bytes::from(history))).chain(live));

    let headers = [
        (
            header::CONTENT_TYPE.as_str(),
            "application/octet-stream".to_owned(),
        ),
        ("x-session-offset", start.to_string()),
        ("x-session-epoch", epoch.to_string()),
    ];

    (headers, body).into_response()
}

#[derive(Deserialize)]
struct AppendQuery {
    offset: u64,
}

/// Appends a batch of records if `offset` is where the session currently ends. Otherwise `409`, with
/// the current length as the body, so the publisher can resume from there.
async fn append(
    State(sessions): State<Arc<Sessions>>,
    Path(id): Path<String>,
    Query(query): Query<AppendQuery>,
    batch: Bytes,
) -> Response {
    if !valid_id(&id) {
        return (StatusCode::BAD_REQUEST, "not a session id\n").into_response();
    }

    let session = match sessions.open(&id) {
        Ok(session) => session,
        Err(e) => return internal(&e),
    };

    match session.append(query.offset, batch) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(AppendError::Conflict(len)) => (StatusCode::CONFLICT, len.to_string()).into_response(),
        Err(AppendError::Io(e)) => internal(&e),
    }
}

fn internal(e: &std::io::Error) -> Response {
    tracing::error!("{e}");
    (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}\n")).into_response()
}
