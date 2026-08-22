use std::net::SocketAddr;

use tokio::sync::mpsc::Sender;

use maviola::asnc::node::Event;
use maviola::asnc::prelude::*;
use maviola::error::CoreError;
use maviola::prelude::{MavLinkId, Network, Node, UdpServer, V2};

use crate::links::LinkId;

async fn listen(addr: SocketAddr, sender: Sender<(LinkId, Event<V2>)>) -> Result<(), CoreError> {
    let network = Network::asnc().add_connection(UdpServer::new(addr)?);

    let node = Node::asnc::<V2>()
        .id(MavLinkId::new(
            crate::GROUND_STATION_SYSTEM_ID,
            crate::GROUND_STATION_COMPONENT_ID,
        ))
        .connection(network)
        .build()
        .await?;

    tracing::info!("Listening on {:?}.", addr);

    let mut events = node.events().unwrap();
    while let Some(event) = events.next().await {
        let _ = sender.send((LinkId::UdpServer(addr), event)).await;
    }

    Ok(())
}

pub async fn run(addr: SocketAddr, sender: Sender<(LinkId, Event<V2>)>) {
    if let Err(e) = listen(addr, sender).await {
        tracing::error!("Failed to listen on {addr:?}: {e:?}");
    }
}
