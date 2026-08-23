use std::time::Duration;

use crate::Core;
use crate::links::LinkId;

use tokio::sync::mpsc::Sender;

use maviola::asnc::node::Event;
use maviola::asnc::prelude::*;
use maviola::error::CoreError;
use maviola::prelude::{MavLinkId, Network, Node, RetryStrategy, SerialPort, V2};
use tokio_serial::SerialPortType;

async fn connect(port: String, sender: Sender<(LinkId, Event<V2>)>) -> Result<(), CoreError> {
    let network = Network::asnc()
        .add_connection(SerialPort::new(&port, 115_200)?)
        .retry(RetryStrategy::Attempts(10, Duration::from_millis(500)))
        .stop_on_node_down(true);

    let node = Node::asnc::<V2>()
        .id(MavLinkId::new(
            crate::GROUND_STATION_SYSTEM_ID,
            crate::GROUND_STATION_COMPONENT_ID,
        ))
        .connection(network)
        .build()
        .await?;

    tracing::info!("Connected to {port}.");

    let mut events = node.events().unwrap();
    while let Some(event) = events.next().await {
        let _ = sender.send((LinkId::SerialPort(port.clone()), event)).await;
    }

    Ok(())
}

pub async fn run(port: String, sender: Sender<(LinkId, Event<V2>)>) {
    if let Err(e) = connect(port.clone(), sender).await {
        tracing::error!("Failed to connect to {port:?}: {e:?}");
    } else {
        tracing::error!("Connection to {port:?} lost.");
    }
}

pub async fn autoconnect(core: Core) {
    let mut last: Vec<tokio_serial::SerialPortInfo> = Vec::new();

    loop {
        match tokio_serial::available_ports() {
            Ok(ports) => {
                for new in ports.iter().filter(|p| !last.contains(p)) {
                    let SerialPortType::UsbPort(usb_info) = &new.port_type else {
                        continue;
                    };

                    tracing::info!(
                        port = new.port_name,
                        vid = usb_info.vid,
                        pid = usb_info.pid,
                        manufacturer = usb_info.manufacturer,
                        product = usb_info.product,
                        serial_number = usb_info.serial_number,
                        "New USB serial port detected."
                    );

                    core.add_link(LinkId::SerialPort(new.port_name.clone()));
                }

                last = ports;
            }
            Err(e) => {
                tracing::error!("Failed to enumerate serial ports: {e:?}");
            }
        }

        crate::time::sleep(Duration::from_millis(500)).await;
    }
}
