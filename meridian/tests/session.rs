//! Against a real service: raw appends and reads, then a ground station's publisher and reader.

use std::time::Duration;

use chrono::Utc;
use nadir_core::mav::dialects::common::messages::{CanFrame, Heartbeat};
use nadir_core::mav::{Endpoint, Frame, MavLinkId, V2};
use nadir_core::publish::Publisher;
use nadir_core::{Origin, SessionAddr, Source, tlog};

/// The service's URL, and the directory it stores sessions in, which has to outlive it.
async fn serve() -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(tokio::spawn(meridian::serve(
        listener,
        dir.path().to_owned(),
    )));

    (format!("http://{addr}"), dir)
}

fn endpoint() -> Endpoint<V2> {
    Endpoint::new(MavLinkId {
        system: 0x2a,
        component: 1,
    })
}

fn heartbeat(endpoint: &Endpoint<V2>) -> Frame<V2> {
    endpoint.next_frame(&Heartbeat::default()).unwrap()
}

fn records(endpoint: &Endpoint<V2>, count: usize) -> Vec<u8> {
    (0..count)
        .flat_map(|_| tlog::record(Utc::now(), &heartbeat(endpoint)).unwrap())
        .collect()
}

async fn eventually(condition: impl Fn() -> bool) -> bool {
    for _ in 0..200 {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    false
}

#[tokio::test]
async fn an_append_must_follow_on_from_the_end() {
    let (server, _dir) = serve().await;
    let client = reqwest::Client::new();
    let url = format!("{server}/sessions/s");

    let first = client.post(format!("{url}?offset=0")).body("abc").send();
    assert_eq!(first.await.unwrap().status(), 204);

    let stale = client.post(format!("{url}?offset=0")).body("def").send();
    let stale = stale.await.unwrap();
    assert_eq!(stale.status(), 409);
    assert_eq!(stale.text().await.unwrap(), "3");

    let next = client.post(format!("{url}?offset=3")).body("def").send();
    assert_eq!(next.await.unwrap().status(), 204);

    let listing = client.get(format!("{server}/sessions")).send();
    assert_eq!(listing.await.unwrap().text().await.unwrap(), "s\t6\t0\n");
}

#[tokio::test]
async fn a_reader_gets_the_history_then_the_live_tail() {
    let (server, _dir) = serve().await;
    let client = reqwest::Client::new();
    let url = format!("{server}/sessions/s");
    let endpoint = endpoint();

    let history = records(&endpoint, 3);
    let post = client.post(format!("{url}?offset=0")).body(history.clone());
    post.send().await.unwrap();

    let mut response = client.get(&url).send().await.unwrap();
    let mut read = Vec::new();
    while read.len() < history.len() {
        read.extend_from_slice(&response.chunk().await.unwrap().unwrap());
    }
    assert_eq!(read, history);

    let live = records(&endpoint, 2);
    let post = client.post(format!("{url}?offset={}", history.len()));
    post.body(live.clone()).send().await.unwrap();

    read.clear();
    while read.len() < live.len() {
        read.extend_from_slice(&response.chunk().await.unwrap().unwrap());
    }
    assert_eq!(read, live);
}

#[tokio::test]
async fn a_reader_resumes_only_within_the_same_session() {
    let (server, _dir) = serve().await;
    let client = reqwest::Client::new();
    let url = format!("{server}/sessions/s");

    let post = client.post(format!("{url}?offset=0")).body("abcdef");
    post.send().await.unwrap();

    let first = client.get(&url).send().await.unwrap();
    let epoch = first.headers()["x-session-epoch"]
        .to_str()
        .unwrap()
        .to_owned();

    let resumed = client.get(format!("{url}?offset=4&epoch={epoch}"));
    let mut resumed = resumed.send().await.unwrap();
    assert_eq!(resumed.headers()["x-session-offset"], "4");
    assert_eq!(resumed.chunk().await.unwrap().unwrap(), "ef");

    let restarted = client.get(format!("{url}?offset=4&epoch=1")).send();
    let mut restarted = restarted.await.unwrap();
    assert_eq!(restarted.headers()["x-session-offset"], "0");
    assert_eq!(restarted.chunk().await.unwrap().unwrap(), "abcdef");
}

#[tokio::test]
async fn an_unknown_session_is_not_found() {
    let (server, _dir) = serve().await;

    let response = reqwest::get(format!("{server}/sessions/nobody"))
        .await
        .unwrap();

    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn a_published_system_reaches_a_reader() {
    let (server, _dir) = serve().await;
    let addr = SessionAddr::new(server, "test station", 0x2a);
    let endpoint = endpoint();

    let publisher = Publisher::spawn(addr.clone());
    for _ in 0..10 {
        publisher.publish(Utc::now(), &heartbeat(&endpoint));
    }
    let can = endpoint.next_frame(&CanFrame::default()).unwrap();
    publisher.publish(Utc::now(), &can);

    let source = Source::stream(&addr, || {});
    let Origin::Stream(progress) = &source.origin else {
        unreachable!();
    };

    assert!(eventually(|| progress.records() == 10).await);
    assert!(source.system(0x2a).is_some());

    for _ in 0..5 {
        publisher.publish(Utc::now(), &heartbeat(&endpoint));
    }

    assert!(eventually(|| progress.records() == 15).await);
    assert_eq!(progress.errors(), 0);
    assert!(addr.id.starts_with("test-station-2a-"));

    progress.cancel();
}
