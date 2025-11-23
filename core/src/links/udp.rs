use std::net::SocketAddr;

use tokio::sync::mpsc::Sender;

use maviola::asnc::node::Event;
use maviola::asnc::prelude::*;
use maviola::prelude::V2;
use maviola::prelude::*;

use crate::links::LinkId;

pub async fn run(addr: SocketAddr, sender: Sender<(LinkId, Event<V2>)>) {
    let network = Network::asnc().add_connection(UdpServer::new(addr).unwrap());

    let node = Node::asnc::<V2>()
        .id(MavLinkId::new(
            crate::GROUND_STATION_SYSTEM_ID,
            crate::GROUND_STATION_COMPONENT_ID,
        ))
        .connection(network)
        .build()
        .await
        .unwrap();

    tracing::info!("Listening on {:?}.", addr);

    let mut events = node.events().unwrap();
    while let Some(event) = events.next().await {
        let _ = sender.send((LinkId::UdpServer(addr), event)).await;
    }
}
