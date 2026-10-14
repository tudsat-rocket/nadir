use std::time::Duration;

use mavspec::rust::dialects::{
    Common,
    common::{
        enums::MavCmd,
        messages::{CameraInformation, CommandLong},
    },
};
use tokio::sync::broadcast::{Receiver, error::RecvError};

use crate::System;
use crate::time::Instant;

/// Requests `CAMERA_INFORMATION` while a camera reports status but has not described itself.
/// Autopilot-attached cameras cannot be asked for individually, so one request covers all of them.
pub async fn request_camera_information(system: System, mut message_rx: Receiver<Common>) {
    const RETRY_AFTER: Duration = Duration::from_secs(5);
    const MAX_ATTEMPTS: usize = 5;

    let mut last_request: Option<Instant> = None;
    let mut attempts = 0;

    while attempts < MAX_ATTEMPTS {
        let status = match message_rx.recv().await {
            Ok(Common::CameraCaptureStatus(status)) => status,
            Err(RecvError::Closed) => return,
            _ => continue,
        };

        let described = system
            .last_instance_message::<CameraInformation>(status.camera_device_id.into())
            .is_ok();
        if described || last_request.is_some_and(|t| t.elapsed() < RETRY_AFTER) {
            continue;
        }

        system.send_message(&CommandLong {
            target_system: system.system_id,
            target_component: 0x01,
            command: MavCmd::RequestMessage,
            param1: CameraInformation::ID as f32,
            ..Default::default()
        });
        last_request = Some(Instant::now());
        attempts += 1;
    }
}
