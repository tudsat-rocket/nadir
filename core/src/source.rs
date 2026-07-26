//! A set of systems and the message store they share.
//!
//! [`Core`](crate::Core) owns the links; a [`Source`] owns what arrives over them. Everything above
//! this layer - the protocol state machines, the database queries, the whole GUI - only ever reads
//! a source, and needs nothing else from `Core`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use tokio::sync::{broadcast, mpsc};

use maviola::asnc::node::Callback;
use maviola::prelude::{Frame, V2};
use maviola::protocol::SystemId;
use maviola::protocol::dialects::{Ardupilotmega, Common};
use mavspec::rust::dialects::common::enums::MavSeverity;

use db::Db;

use crate::system::System;
use crate::tlog;

#[derive(Clone)]
pub struct Source {
    pub db: Db,
    pub tlog: tlog::Writer,
    pub systems: Arc<Mutex<HashMap<SystemId, System>>>,
    /// Zero point of the plot time axis, fixed for the life of the source.
    pub plot_origin: DateTime<Utc>,
    can_proxy: Option<(
        mpsc::Sender<socketcan::CanFrame>,
        broadcast::Sender<socketcan::CanFrame>,
    )>,
}

impl Source {
    /// The source the links feed.
    pub(crate) fn live(
        can_proxy: Option<(
            mpsc::Sender<socketcan::CanFrame>,
            broadcast::Sender<socketcan::CanFrame>,
        )>,
    ) -> Self {
        Self {
            db: Db::init(),
            tlog: tlog::Writer::spawn(),
            systems: Arc::new(Mutex::new(HashMap::new())),
            plot_origin: Utc::now(),
            can_proxy,
        }
    }

    pub fn known_system_ids(&self) -> Vec<SystemId> {
        let mut system_ids: Vec<SystemId> = self.systems.lock().unwrap().keys().copied().collect();
        system_ids.sort_unstable();
        system_ids.dedup();
        system_ids
    }

    pub fn system(&self, id: SystemId) -> Option<System> {
        self.systems.lock().unwrap().get(&id).cloned()
    }

    /// Files one frame: decodes it, stores it, and hands it to its system's protocol tasks.
    pub(crate) fn ingest(&self, frame: &Frame<V2>, callback: &Callback<V2>) {
        let mut systems = self.systems.lock().unwrap();
        let system_id = frame.system_id();

        let system = systems.entry(system_id).or_insert_with(|| {
            System::new(
                system_id,
                self.db.clone(),
                self.tlog.clone(),
                callback.clone(),
                self.can_proxy.clone(),
            )
        });

        if let Ok(message) = frame.decode::<Common>() {
            if let Common::Statustext(inner) = &message {
                log_statustext(frame, inner.severity, &inner.text);
            }

            self.write(frame, &message);
            system.notify_of_common_message(message, frame, callback);
        } else if let Ok(message) = frame.decode::<Ardupilotmega>() {
            self.write(frame, &message);
            system.notify_of_frame(frame, callback);
        } else if let Ok(message) = frame.decode::<rapid_dialect::Rapid>() {
            self.write(frame, &message);
            system.notify_of_frame(frame, callback);
        }
    }

    fn write<M: db::MessageExt>(&self, frame: &Frame<V2>, message: &M) {
        if let Err(e) = self
            .db
            .write_message(frame.system_id(), frame.component_id(), message)
        {
            tracing::error!("Failed to process message: {e:?}");
        }
    }
}

// TODO: move this to its own protocol task as well to get it out of here
fn log_statustext(frame: &Frame<V2>, severity: MavSeverity, text: &[u8]) {
    let system_id = frame.system_id();
    let component_id = frame.component_id();
    let text = String::from_utf8_lossy(text);

    match severity {
        MavSeverity::Debug => tracing::debug!(system_id, component_id, "{}", &text),
        MavSeverity::Info | MavSeverity::Notice => {
            tracing::info!(system_id, component_id, "{}", &text);
        }
        MavSeverity::Warning => tracing::warn!(system_id, component_id, "{}", &text),
        MavSeverity::Error
        | MavSeverity::Alert
        | MavSeverity::Critical
        | MavSeverity::Emergency => tracing::error!(system_id, component_id, "{}", &text),
    }
}
