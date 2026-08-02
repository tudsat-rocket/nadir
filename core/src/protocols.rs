use std::fmt::Debug;
use std::time::Duration;

use crate::mav::{ComponentId, Message};
use crate::time::timeout;
use db::MessageExt;
use mavspec::rust::dialects::{Common, common::enums::MavResult};

use crate::System;

#[cfg(not(target_arch = "wasm32"))]
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
    progress_cb: Option<Box<dyn Fn(usize, usize) + Send>>,
) -> Result<Vec<M>, MavResult> {
    const MAX_RETRIES: usize = 3;
    const REQUEST_TIMEOUT: Duration = Duration::from_millis(1000);

    let message_id = M::default().id();
    let protocol = mavspec::definitions::protocol();
    let common = protocol.get_dialect_by_name("common").unwrap();
    let msg_spec = common.get_message_by_id(message_id).unwrap();
    let message_name = msg_spec.name();

    let system_id = system.system_id;

    tracing::Span::current().record("system_id", system_id);
    tracing::Span::current().record("component_id", component_id);
    tracing::Span::current().record("message_name", message_name);

    // First, we send our initial request, to which the system should response with all items.
    // We find out how many items there are in total with the first item received.
    // We stop when we haven't received anything in a while and move on the next phase.
    tracing::debug!("Sending initial request.");
    let initial_request = M::initial_request(system.system_id, component_id);
    system.send_message(&initial_request);

    let mut number_items: Option<usize> = None;
    let mut items: Vec<Option<M>> = Vec::new();
    let mut received: usize = 0;

    // We retry the initial request a couple of times, but only if we received no response at all.
    for i in 0..MAX_RETRIES {
        received = 0;

        while let Ok(item) = timeout(REQUEST_TIMEOUT, recv_item::<M>(message_rx)).await {
            let count = item.count();

            // This is our first message, populate our item vec with empty slots.
            if number_items.is_none() {
                number_items = Some(count);
                items = vec![None; count];
            }

            if let Some(opt) = items.get_mut(item.index()) {
                // If we get the same param multiple times for some reason, don't increment.
                if opt.is_none() {
                    received += 1;
                }

                *opt = Some(item);
            }

            if let Some(cb) = progress_cb.as_ref() {
                cb(received, count);
            }

            // If we're full, we don't need to wait for the timeout.
            if number_items.is_some_and(|num| num == received) {
                break;
            }
        }

        // If we got a single message at all, we are done in this phase. If not, we retry.
        // Maybe our command was lost.
        if number_items.is_some() {
            break;
        } else if i < MAX_RETRIES - 1 {
            tracing::debug!("No items received, retrying.");
        }
    }

    // Despite our retries, we got nothing at all, abort.
    let Some(num_items) = number_items else {
        tracing::error!("No response to request.");
        return Err(MavResult::Failed);
    };

    tracing::debug!("Got {}/{} in discovery phase.", received, num_items);

    // We know have an exact list of items we don't have. For each missing item we request that
    // exact item.
    let missing = items
        .iter_mut()
        .enumerate()
        .filter(|(_i, item)| item.is_none());
    'outer: for (i, missing_item) in missing {
        for _retry in 0..MAX_RETRIES {
            tracing::debug!("Rerequesting {}/{}.", i, num_items);

            let request = M::specific_request(system_id, component_id, i);
            system.send_message(&request);

            if let Ok(item) = timeout(REQUEST_TIMEOUT, recv_item::<M>(message_rx)).await
                && item.index() == i
            {
                received += 1;
                *missing_item = Some(item);

                if let Some(cb) = progress_cb.as_ref() {
                    cb(received, num_items);
                }

                continue 'outer;
            }
        }
    }

    if num_items == received {
        tracing::info!("Successfully gathered all {} items.", num_items);
        Ok(items.into_iter().map(|opt| opt.unwrap()).collect())
    } else {
        let missing = num_items - received;
        tracing::error!("Failed to gather all items (missing {}).", missing);
        Err(MavResult::Failed)
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
