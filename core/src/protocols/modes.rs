use std::time::Duration;

use maviola::protocol::ComponentId;
use mavspec::rust::dialects::{
    Common,
    common::{
        enums::MavCmd,
        messages::{AvailableModes, CommandLong},
    },
};
use tokio::time::timeout;

use crate::System;

pub async fn discover_available_modes(
    system: System,
    component_id: ComponentId,
    mut message_rx: tokio::sync::broadcast::Receiver<Common>,
) {
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
