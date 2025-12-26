use std::time::Duration;

use maviola::protocol::ComponentId;
use mavspec::rust::dialects::Common;
use mavspec::rust::dialects::ardupilotmega::enums::CopterMode;
use mavspec::rust::dialects::common::{
    enums::{MavAutopilot, MavCmd, MavModeProperty, MavStandardMode, MavType},
    messages::{AvailableModes, CommandLong, Heartbeat},
};
use tokio::time::timeout;

use crate::System;

pub async fn discover_available_modes(
    system: System,
    component_id: ComponentId,
    mut message_rx: tokio::sync::broadcast::Receiver<Common>,
) {
    let heartbeat = loop {
        if let Ok(heartbeat) = system.last_message::<Heartbeat>() {
            break heartbeat;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    // Arducopter does not support this protocol for some reason
    if heartbeat.autopilot == MavAutopilot::Ardupilotmega && heartbeat.type_ != MavType::FixedWing {
        let modes = CopterMode::entries()
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
            .collect();

        *system.available_modes.lock().unwrap() = Some(modes);
        return;
    }

    tracing::debug!(system_id = system.system_id, "Starting mode discovery.");

    loop {
        system.send_message(&CommandLong {
            target_system: system.system_id,
            target_component: component_id,
            command: MavCmd::RequestMessage,
            param1: AvailableModes::ID as f32,
            ..Default::default()
        });

        let recv_modes = async {
            let mut number_modes: Option<usize> = None;
            let mut modes = Vec::new();
            while number_modes.is_none_or(|num| modes.len() < num) {
                if let Ok(Common::AvailableModes(mode_info)) = message_rx.recv().await {
                    number_modes = Some(mode_info.number_modes as usize);
                    modes.push(mode_info);
                }
            }
            modes
        };

        match timeout(Duration::from_millis(5000), recv_modes).await {
            Ok(modes) => {
                *system.available_modes.lock().unwrap() = Some(modes);
                break;
            }
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(10000)).await;
            }
        }
    }
}
