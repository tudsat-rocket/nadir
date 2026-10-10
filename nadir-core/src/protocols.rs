use std::fmt::Debug;
use std::time::Duration;

use crate::mav::{ComponentId, Message};
use crate::time::timeout;
use mavspec::rust::dialects::Common;
use nadir_store::MessageExt;

use crate::System;

#[cfg(target_os = "linux")]
pub mod can;
pub mod heartbeat;
pub mod intervals;
pub mod logs;
pub mod modes;
pub mod params;

/// Trait shared by gatherable messages.
///
/// Many `MAVLink` services / protocols follow a pattern where a GCS can request a number of items
/// using either a message to request all such items, or a message to request a specific one.
///
/// In order to allow a generic implementation of such a service, this trait should be implemented
/// for such messages.
///
/// Examples include:
///     - `AVAILABLE_MODES`: <https://mavlink.io/en/services/standard_modes.html>
///     - `PARAM_VALUE`: <https://mavlink.io/en/services/parameter.html>
///     - `LOG_ENTRY`: <https://mavlink.io/en/messages/common.html#LOG_REQUEST_LIST>
///
/// Might have to be adjusted for mission download.
pub trait Gatherable: Message + MessageExt + Sized {
    type InitialRequest: Message + MessageExt + Debug;
    type SpecificRequest: Message + MessageExt + Debug;

    /// How many specific requests may be in flight at once. Most responders keep a single slot
    /// that each request overwrites (`ArduPilot` and PX4 for `AVAILABLE_MODES` and log lists).
    const BATCH: usize = 1;

    /// Index of itself in the complete collection.
    fn index(&self) -> usize;

    /// Total size of the complete collection.
    fn count(&self) -> usize;

    /// Filter function for extraction Self from a stream of messages of type [`Common`].
    fn unpack(msg: Common) -> Option<Self>;

    /// Mavlink Message that promts the Vehicle to send a complete collection of elements of type
    /// Self.
    fn initial_request(system_id: u8, component_id: u8) -> Self::InitialRequest;

    /// Mavlink Message that promts the Vehicle to send a range of the complete collection of type
    /// Self.
    fn specific_request(system_id: u8, component_id: u8, index: usize) -> Self::SpecificRequest;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherError {
    NoResponse,
    Incomplete { received: usize, total: usize },
}

impl std::fmt::Display for GatherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoResponse => write!(f, "No response"),
            Self::Incomplete { received, total } => {
                write!(f, "Missing {} of {total}", total - received)
            }
        }
    }
}

/// The items of a collection received so far, by index.
struct Gathered<M> {
    items: Vec<Option<M>>,
    received: usize,
}

impl<M: Gatherable + Debug + Clone + Default> Gathered<M> {
    /// Sizes the collection from the first item. Later items of a different size are dropped,
    /// since the broadcast carries no component id to tell their senders apart.
    fn accept(&mut self, item: M) {
        if self.items.is_empty() {
            self.items = vec![None; item.count()];
        }

        if item.count() != self.items.len() {
            return;
        }

        if let Some(slot) = self.items.get_mut(item.index()) {
            if slot.is_none() {
                self.received += 1;
            }
            *slot = Some(item);
        }
    }

    fn missing(&self) -> Vec<usize> {
        (0..self.items.len())
            .filter(|&i| self.items[i].is_none())
            .collect()
    }

    /// Accepts items of any index until `done` holds or the link has been quiet for a while.
    async fn receive_until(
        &mut self,
        message_rx: &mut tokio::sync::broadcast::Receiver<Common>,
        progress_cb: Option<&(dyn Fn(usize, usize) + Send + Sync)>,
        done: impl Fn(&Self) -> bool,
    ) {
        const IDLE_TIMEOUT: Duration = Duration::from_secs(1);

        while !done(self)
            && let Ok(item) = timeout(IDLE_TIMEOUT, recv_item::<M>(message_rx)).await
        {
            self.accept(item);

            if let Some(cb) = progress_cb {
                cb(self.received, self.items.len());
            }
        }
    }
}

/// Gathers a message implementing the Gatherable trait.
#[tracing::instrument(
    name = "gather",
    skip_all,
    fields(system_id, component_id, message_name)
)]
pub(crate) async fn gather<M: Gatherable + Debug + Clone + Default>(
    system: &System,
    component_id: ComponentId,
    message_rx: &mut tokio::sync::broadcast::Receiver<Common>,
    progress_cb: Option<Box<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<Vec<M>, GatherError> {
    const MAX_RETRIES: usize = 3;

    let message_id = M::default().id();
    let protocol = mavspec::definitions::protocol();
    let common = protocol.get_dialect_by_name("common").unwrap();
    let msg_spec = common.get_message_by_id(message_id).unwrap();
    let message_name = msg_spec.name();

    let system_id = system.system_id;

    tracing::Span::current().record("system_id", system_id);
    tracing::Span::current().record("component_id", component_id);
    tracing::Span::current().record("message_name", message_name);

    let progress_cb = progress_cb.as_deref();
    let mut gathered = Gathered::<M> {
        items: Vec::new(),
        received: 0,
    };

    // The system should respond to the initial request with all items, the first of which tells
    // us how many there are. Retry only if nothing arrives at all, the request may have been lost.
    for attempt in 0..MAX_RETRIES {
        tracing::debug!("Sending initial request.");
        system.send_message(&M::initial_request(system_id, component_id));

        gathered
            .receive_until(message_rx, progress_cb, |g| {
                !g.items.is_empty() && g.received == g.items.len()
            })
            .await;

        if !gathered.items.is_empty() {
            break;
        } else if attempt < MAX_RETRIES - 1 {
            tracing::debug!("No items received, retrying.");
        }
    }

    if gathered.items.is_empty() {
        tracing::error!("No response to request.");
        return Err(GatherError::NoResponse);
    }

    let total = gathered.items.len();
    tracing::debug!("Got {}/{total} in discovery phase.", gathered.received);

    // Request the missing items in batches. The initial stream may still be arriving, so every
    // item counts, not just the requested ones. Give up only once rounds stop making progress.
    let mut fruitless_rounds = 0;
    while gathered.received < total && fruitless_rounds < MAX_RETRIES {
        let received_before = gathered.received;
        let requested: Vec<_> = gathered.missing().into_iter().take(M::BATCH).collect();

        tracing::debug!("Rerequesting {requested:?} of {total}.");
        for &index in &requested {
            system.send_message(&M::specific_request(system_id, component_id, index));
        }

        gathered
            .receive_until(message_rx, progress_cb, |g| {
                requested.iter().all(|&i| g.items[i].is_some())
            })
            .await;

        if gathered.received == received_before {
            fruitless_rounds += 1;
        } else {
            fruitless_rounds = 0;
        }
    }

    if gathered.received == total {
        tracing::info!("Successfully gathered all {total} items.");
        Ok(gathered.items.into_iter().flatten().collect())
    } else {
        let received = gathered.received;
        tracing::error!("Failed to gather all items (missing {}).", total - received);
        Err(GatherError::Incomplete { received, total })
    }
}

async fn recv_item<M: Gatherable + Debug + Clone + Default>(
    message_rx: &mut tokio::sync::broadcast::Receiver<Common>,
) -> M {
    loop {
        let Ok(msg) = message_rx.recv().await else {
            continue;
        };

        if let Some(item) = M::unpack(msg) {
            return item;
        }
    }
}
