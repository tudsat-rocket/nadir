use std::sync::Arc;
use std::{thread::sleep, time::Duration};

use tokio::sync::broadcast::Sender;
use tokio::{sync::mpsc::Receiver, task};

use socketcan::CanAddr;
use socketcan::tokio::CanSocket;

use tracing::{debug, trace, warn};

pub fn spawn_can_proxy(
    tx_receiver: Receiver<socketcan::CanFrame>,
    rx_publisher: Sender<socketcan::CanFrame>,
) {
    let mut can_socket = CanAddr::from_iface("vcan0").and_then(|addr| CanSocket::open_addr(&addr));
    if can_socket.is_err() {
        warn!("could not connect to SocketCan socket, retrying");
    }
    while can_socket.is_err() {
        debug!("could not connect to SocketCan socket, retrying");
        sleep(Duration::from_secs(3));
        can_socket = CanAddr::from_iface("vcan0").and_then(|addr| CanSocket::open_addr(&addr));
    }

    match CanAddr::from_iface("vcan0").and_then(|addr| CanSocket::open_addr(&addr)) {
        Ok(can_socket) => {
            let shared_sock = Arc::new(can_socket);
            let receiver_sock = shared_sock.clone();
            task::spawn(socketcan_writer(receiver_sock, tx_receiver));
            task::spawn(socketcan_reader(shared_sock, rx_publisher));
        }
        Err(..) => unreachable!(),
    }
}

/// Receives CAN frames from all connected `MAVLink` systems and writes them to our socketcan socket
async fn socketcan_writer(socket: Arc<CanSocket>, mut receiver: Receiver<socketcan::CanFrame>) {
    while let Some(can_frame) = receiver.recv().await {
        trace!("writing frame to can socket");
        // NOTE: ignore error for now
        let _ = socket.write_frame(can_frame).await;
    }
}

/// Publishes any CAN frames read from the socketcan socket to all connected `MAVLink` systems
async fn socketcan_reader(socket: Arc<CanSocket>, publisher: Sender<socketcan::CanFrame>) {
    loop {
        if let Ok(can_frame) = socket.read_frame().await {
            let _ = publisher.send(can_frame);
        }
    }
}
