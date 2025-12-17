use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::Sender;

use maviola::asnc::node::Event;
use maviola::prelude::*;

use crate::stats::LinkStats;
use crate::{GROUND_STATION_COMPONENT_ID, GROUND_STATION_SYSTEM_ID};

pub mod tcp;
pub mod udp;
pub mod usb;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum LinkId {
    TcpClient(SocketAddr),
    UdpServer(SocketAddr),
    SerialPort(String),
}

#[derive(Clone)]
pub struct Link {
    pub id: LinkId,
    //event_loop_task: tokio::task::JoinHandle<()>,
    pub endpoint: Arc<Mutex<Endpoint<V2>>>,
    pub stats: LinkStats,
}

impl LinkId {
    pub fn spawn(&self, sender: Sender<(LinkId, Event<V2>)>) -> Link {
        let _event_loop_task = match self {
            Self::TcpClient(addr) => tokio::spawn(tcp::run(*addr, sender)),
            Self::UdpServer(addr) => tokio::spawn(udp::run(*addr, sender)),
            Self::SerialPort(port) => tokio::spawn(usb::run(port.clone(), sender)),
        };

        Link {
            id: self.clone(),
            //event_loop_task,
            endpoint: Arc::new(Mutex::new(Endpoint::new(MavLinkId {
                system: GROUND_STATION_SYSTEM_ID,
                component: GROUND_STATION_COMPONENT_ID,
            }))),
            stats: LinkStats::default(),
        }
    }
}
