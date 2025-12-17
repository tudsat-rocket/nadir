use std::time::Duration;

use mavspec::rust::dialects::{
    Common,
    common::{
        enums::{MavCmd, MavResult},
        messages::{
            Attitude, AutopilotVersion, CommandLong, GlobalPositionInt, LinkNodeStatus,
            LocalPositionNed, ServoOutputRaw, VfrHud,
        },
    },
};

use crate::System;

pub async fn request_message_intervals(
    system: System,
    mut message_rx: tokio::sync::broadcast::Receiver<Common>,
) {
    const INTERVALS: [(u32, u32); 7] = [
        (Attitude::ID, 100_000),
        (VfrHud::ID, 100_000),
        (LocalPositionNed::ID, 100_000),
        (GlobalPositionInt::ID, 200_000),
        (ServoOutputRaw::ID, 200_000),
        (LinkNodeStatus::ID, 2_000_000),
        (AutopilotVersion::ID, 5_000_000),
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
