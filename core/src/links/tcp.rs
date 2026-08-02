use std::net::SocketAddr;
use std::time::Duration;

use tokio::sync::mpsc::Sender;

use maviola::asnc::node::Event;
use maviola::asnc::prelude::*;
use maviola::error::CoreError;
use maviola::prelude::{MavLinkId, Network, Node, RetryStrategy, TcpClient, V2};

use crate::links::LinkId;

async fn connect(addr: SocketAddr, sender: Sender<(LinkId, Event<V2>)>) -> Result<(), CoreError> {
    let network = Network::asnc()
        .add_connection(TcpClient::new(addr)?)
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

    tracing::info!("Connected to {:?}", addr);

    let mut events = node.events().unwrap();
    while let Some(event) = events.next().await {
        let _ = sender.send((LinkId::TcpClient(addr), event)).await;
    }

    Ok(())
}

pub async fn run(addr: SocketAddr, sender: Sender<(LinkId, Event<V2>)>) {
    let mut log_failure = true;
    loop {
        match connect(addr, sender.clone()).await {
            Ok(()) => {
                tracing::warn!("Connection to {addr:?} lost, will keep attempting to reconnect.");
            }
            Err(e) if log_failure => {
                tracing::error!("Failed to connect to {addr:?}: {e:?}, will keep trying.");
                log_failure = false;
            }
            _ => {
                crate::time::sleep(Duration::from_millis(5000)).await;
            }
        }
    }
}
