use std::time::Duration;

use crate::time::sleep;

use mavspec::rust::dialects::common::{
    enums::{MavAutopilot, MavModeFlag, MavState, MavType},
    messages::Heartbeat,
};

use crate::System;

pub async fn send_heartbeats(system: System) {
    loop {
        system.send_message(&Heartbeat {
            type_: MavType::Gcs,
            autopilot: MavAutopilot::Generic,
            base_mode: MavModeFlag::empty(),
            custom_mode: 0x00,
            system_status: MavState::Active,
            mavlink_version: 2,
        });

        sleep(Duration::from_millis(500)).await;
    }
}
