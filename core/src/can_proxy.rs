use std::sync::Arc;
use std::{thread::sleep, time::Duration};

use mavspec::rust::dialects::common::messages;
use socketcan::tokio::CanSocket;
use socketcan::{CanAddr, EmbeddedFrame, Id};
use tokio::{sync::mpsc::Receiver, task};
use tracing::{trace, warn};

pub async fn spawn_can_proxy(
    receive_can: Receiver<socketcan::CanFrame>,
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
            task::spawn(async move { can_receiver(receiver_sock, receive_can).await });
            task::spawn(async move { can_sender(shared_sock, core).await });
        }
        Err(..) => unreachable!(),
    }
    Ok(())
}

// Receives can messages from the main task and writes them to the socket.
async fn can_receiver(socket: Arc<CanSocket>, mut receiver: Receiver<socketcan::CanFrame>) {
    while let Some(can_frame) = receiver.recv().await {
        trace!("writing frame to can socket");
        // NOTE: ignore error for now
        let _ = socket.write_frame(can_frame).await;
    }
}

// TODO: make system gui configurable
/// Sends can messages, which were read on the socket, over mavlink.
/// For now this is hardcoded to Mavlink System 1.
async fn can_sender(socket: Arc<CanSocket>, core: super::Core) {
    while let Ok(can_frame) = socket.read_frame().await {
        trace!("Can frame received via socket");
        // Convert from one can frame type to the other
        let socketcan::CanFrame::Data(can_data_frame) = can_frame else {
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

        // copy data from can_data_frame into data array
        for (i, byte) in can_data_frame
            .data()
            .iter()
            .enumerate()
            .take(can_data_frame.dlc())
        {
            data[i] = *byte
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
            system.send_message(&mavlink_frame);
        }
    }
}
