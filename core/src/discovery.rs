use std::time::Duration;

use maviola::protocol::ComponentId;
use mavspec::rust::dialects::{
    Common,
    common::{
        enums::{MavCmd, MavResult},
        messages::{
            Attitude, AvailableModes, CommandLong, GlobalPositionInt, LocalPositionNed, VfrHud,
        },
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
    let mut modes = Vec::new();

    while number_modes.map(|num| modes.len() < num).unwrap_or(true) {
        match message_rx.recv().await.unwrap() {
            Common::AvailableModes(mode_info) => {
                number_modes = Some(mode_info.number_modes as usize);
                modes.push(mode_info);
            }
            _ => {}
        }
    }

    *system.available_modes.lock().unwrap() = Some(modes);
}

pub async fn request_message_intervals(
    system: System,
    mut message_rx: tokio::sync::broadcast::Receiver<Common>,
) {
    const INTERVALS: [(u32, u32); 4] = [
        (Attitude::ID, 100_000),
        (VfrHud::ID, 100_000),
        (LocalPositionNed::ID, 100_000),
        (GlobalPositionInt::ID, 200_000),
    ];

    loop {
        for (msg_id, rate) in INTERVALS {
            tokio::time::sleep(Duration::from_millis(500)).await;

            system.send_message(&CommandLong {
                target_system: system.system_id,
                target_component: 0x01,
                command: MavCmd::SetMessageInterval,
                param1: msg_id as f32,
                param2: rate as f32,
                ..Default::default()
            });

            let ack_result = tokio::time::timeout(Duration::from_millis(1000), async {
                loop {
                    let msg = message_rx.recv().await;
                    if let Ok(Common::CommandAck(ack)) = msg
                        && ack.command == MavCmd::SetMessageInterval
                    {
                        return ack.result;
                    }
                }
            })
            .await;

            if ack_result.unwrap_or(MavResult::Failed) == MavResult::Unsupported {
                return;
            }
        }

        tokio::time::sleep(Duration::from_millis(10_000)).await;
    }
}
