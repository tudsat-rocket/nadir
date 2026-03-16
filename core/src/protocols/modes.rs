use std::time::Duration;

use maviola::protocol::ComponentId;
use mavspec::rust::dialects::Common;
use mavspec::rust::dialects::ardupilotmega::enums::CopterMode;
use mavspec::rust::dialects::common::{
    enums::{MavAutopilot, MavCmd, MavModeProperty, MavStandardMode, MavType},
    messages::{AvailableModes, CommandLong},
};

use crate::System;
use crate::protocols::{Gatherable, gather};

impl Gatherable for AvailableModes {
    type InitialRequest = CommandLong;
    type SpecificRequest = CommandLong;

    fn index(&self) -> usize {
        // AVAILABLE_MODES uses 1-based indices
        (self.mode_index as usize) - 1
    }

    fn count(&self) -> usize {
        self.number_modes as usize
    }

    fn unpack(msg: Common) -> Option<Self> {
        match msg {
            Common::AvailableModes(inner) => Some(inner),
            _ => None,
        }
    }

    fn initial_request(system_id: u8, component_id: u8) -> Self::InitialRequest {
        CommandLong {
            target_system: system_id,
            target_component: component_id,
            command: MavCmd::RequestMessage,
            param1: AvailableModes::ID as f32,
            param2: 0.0,
            ..Default::default()
        }
    }

    fn specific_request(system_id: u8, component_id: u8, index: usize) -> Self::SpecificRequest {
        CommandLong {
            target_system: system_id,
            target_component: component_id,
            command: MavCmd::RequestMessage,
            param1: AvailableModes::ID as f32,
            param2: (index + 1) as f32,
            ..Default::default()
        }
    }
}

fn arducopter_modes() -> Vec<AvailableModes> {
    CopterMode::entries()
        .enumerate()
        .map(|(i, cm)| {
            let mode_name_str = format!("{cm:?}");
            let mode_name_bytes = mode_name_str.as_bytes();

            let mut mode_name = [0x00; 35];
            mode_name[..(mode_name_bytes.len())].copy_from_slice(mode_name_bytes);

            AvailableModes {
                number_modes: CopterMode::entries().collect::<Vec<_>>().len() as u8,
                mode_index: i as u8,
                standard_mode: MavStandardMode::NonStandard,
                custom_mode: u32::from(cm.value()),
                properties: MavModeProperty::empty(),
                mode_name,
            }
        })
        .collect()
}

pub async fn discover_available_modes(
    system: System,
    component_id: ComponentId,
    mut message_rx: tokio::sync::broadcast::Receiver<Common>,
) {
    // Wait for a heartbeat so we know our firmware, because...
    let heartbeat = loop {
        if let Ok(Common::Heartbeat(hb)) = message_rx.recv().await {
            break hb;
        }
    };

    // Arducopter does not support this protocol for some reason.
    if heartbeat.autopilot == MavAutopilot::Ardupilotmega && heartbeat.type_ != MavType::FixedWing {
        let modes = arducopter_modes();
        *system.available_modes.lock().unwrap() = Some(modes);
        return;
    }

    // Gather AVAILABLE_MODES data.
    tracing::debug!(system_id = system.system_id, "Starting mode discovery.");

    let mut first = true;
    loop {
        match gather(&system, component_id, &mut message_rx, None).await {
            Ok(modes) => {
                *system.available_modes.lock().unwrap() = Some(modes);
                break;
            }
            Err(_res) => {
                if first {
                    tracing::error!("Mode discovery failed, will keep retrying.");
                }

                tokio::time::sleep(Duration::from_millis(10000)).await;
            }
        }

        first = false;
    }
}
