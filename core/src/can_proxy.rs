use std::sync::Arc;
use std::{thread::sleep, time::Duration};

use async_channel::{Receiver, Sender};
use mavspec::rust::dialects::common::messages;
use socketcan::{CanAddr, CanDataFrame, CanFrame, CanSocket, EmbeddedFrame, Id, Socket};
use tokio::task;
use tracing::{info, warn};

pub async fn spawn_can_proxy(
    rx_to_proxy: Receiver<CanFrame>,
    tx_from_proxy: Sender<CanFrame>,
    core: super::Core,
) -> Result<(), ()> {
    let mut can_socket = CanAddr::from_iface("vcan0").and_then(|addr| CanSocket::open_addr(&addr));
    while can_socket.is_err() {
        warn!("could not connect to SocketCan socket, retrying");
        sleep(Duration::from_secs(3));
        can_socket = CanAddr::from_iface("vcan0").and_then(|addr| CanSocket::open_addr(&addr));
    }

    match CanAddr::from_iface("vcan0").and_then(|addr| CanSocket::open_addr(&addr)) {
        Ok(can_socket) => {
            let shared_sock = Arc::new(can_socket);
            let receiver_sock = shared_sock.clone();
            task::spawn(async move { can_receiver(receiver_sock, rx_to_proxy).await });
            task::spawn(async move { can_sender(shared_sock, tx_from_proxy, core).await });
        }
        Err(..) => unreachable!(),
    }
    Ok(())
}

// Receives can messages from the main task and writes them to the socket.
async fn can_receiver(socket: Arc<CanSocket>, receiver: Receiver<CanFrame>) {
    while let Ok(can_frame) = receiver.recv().await {
        // NOTE: ignore error for now
        let _ = socket.write_frame_insist(&can_frame);
    }
}
// Sends can messages, which were read on the socket, to the main task.
async fn can_sender(socket: Arc<CanSocket>, sender: Sender<CanFrame>, core: super::Core) {
    while let Ok(can_frame) = socket.read_frame() {
        info!("Can frame received via socket");
        // Convert from one can frame type to the other
        let CanFrame::Data(can_data_frame) = can_frame else {
            warn!("Non-data frame received via can socket, dropping");
            continue;
        };
        let id = match can_data_frame.id() {
            Id::Standard(id) => id.as_raw() as u32,
            Id::Extended(id) => id.as_raw(),
        };
        if can_data_frame.dlc() > 8 {
            continue;
        }
        let mut data: [u8; 8] = [0; 8];
        for i in 0..can_data_frame.dlc() {
            data[i] = *can_data_frame.data().get(i).unwrap();
        }

        if let Some(system) = core.system(1) {
            let mavlink_frame = messages::CanFrame {
                target_system: 1,
                target_component: 1,
                bus: 1,
                data,
                len: can_data_frame.dlc() as u8,
                id,
            };
            info!("calling system.send_can_message");
            system.send_can_message(mavlink_frame);
        }
        // let _ = sender.send(can_frame).await;
    }
}
