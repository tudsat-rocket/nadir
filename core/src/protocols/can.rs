use std::time::Duration;

use tokio::sync::{broadcast::Receiver, mpsc::Sender};

use mavspec::rust::dialects::{Common, common::messages};

use socketcan::{EmbeddedFrame as _, ExtendedId, Id, StandardId};

use crate::System;

/// Forward `MAVLink` `CAN_FRAME` messages to socketcan writer
pub async fn forward_to_socketcan(
    mut message_rx: Receiver<Common>,
    socketcan_tx: Sender<socketcan::CanFrame>,
) {
    loop {
        let Ok(msg) = message_rx.recv().await else {
            continue;
        };

        let Common::CanFrame(can_frame) = msg else {
            continue;
        };

        tracing::trace!("Forwarding CAN frame to socketcan: {:?}", can_frame);

        let id = if can_frame.id > 0x1FFF_FFFF {
            tracing::warn!("received illegal can id: {}", can_frame.id);
            continue;
        } else if can_frame.id > 0b111_1111_1111 {
            Id::Extended(ExtendedId::new(can_frame.id).unwrap())
        } else {
            Id::Standard(StandardId::new(can_frame.id as u16).unwrap())
        };

        let frame = socketcan::CanFrame::new(id, &can_frame.data[..(can_frame.len as usize)])
            .expect("can frame creation should not have failed");
        let _ = socketcan_tx
            .send_timeout(frame, Duration::from_millis(1))
            .await;
    }
}

/// Receives socketcan CAN frames and passes them on via `MAVLink`
pub async fn subscribe_to_socketcan(
    system: System,
    mut socketcan_rx: Receiver<socketcan::CanFrame>,
) {
    loop {
        let Ok(can_frame) = socketcan_rx.recv().await else {
            continue;
        };

        tracing::trace!("Can frame received via socket");
        // Convert from one can frame type to the other
        let socketcan::CanFrame::Data(can_data_frame) = can_frame else {
            tracing::warn!("Non-data frame received via can socket, dropping");
            continue;
        };

        let id = match can_data_frame.id() {
            Id::Standard(id) => u32::from(id.as_raw()),
            Id::Extended(id) => id.as_raw(),
        };

        if can_data_frame.dlc() > 8 {
            continue;
        }

        let mut data: [u8; 8] = [0; 8];

        // copy data from can_data_frame into data array
        for (i, byte) in can_data_frame
            .data()
            .iter()
            .enumerate()
            .take(can_data_frame.dlc())
        {
            data[i] = *byte;
        }

        let mavlink_frame = messages::CanFrame {
            target_system: 0x04,
            target_component: 0x01,
            bus: 1,
            data,
            len: can_data_frame.dlc() as u8,
            id,
        };
        system.send_message(&mavlink_frame);
    }
}
