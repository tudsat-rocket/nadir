use std::collections::HashMap;

use maviola::{prelude::V2, protocol::ComponentId};
use mavspec::rust::dialects::{
    Common,
    common::{
        enums::MavCmd,
        messages::{AvailableModes, CommandLong},
    },
};

use crate::System;

pub async fn discover_available_modes(
    system: System,
    component_id: ComponentId,
    mut message_rx: tokio::sync::broadcast::Receiver<Common>,
) {
    // TODO: make this more robust, handle timeouts, re-request all/some modes

    tracing::debug!("Starting mode discovery.");

    system.send_message(&CommandLong {
        target_system: system.system_id,
        target_component: component_id,
        command: MavCmd::RequestMessage,
        param1: AvailableModes::ID as f32,
        ..Default::default()
    });

    let mut number_modes: Option<usize> = None;
    let mut modes = HashMap::new();

    while number_modes.map(|num| modes.len() < num).unwrap_or(true) {
        match message_rx.recv().await.unwrap() {
            Common::AvailableModes(mode_info) => {
                number_modes = Some(mode_info.number_modes as usize);
                let name = String::from_utf8_lossy(&mode_info.mode_name);
                modes.insert(mode_info.custom_mode, name.to_string());
            }
            _ => {}
        }
    }

    *system.custom_modes.lock().unwrap() = Some(modes);
}
