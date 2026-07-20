use std::net::SocketAddr;

use tokio::sync::mpsc::Sender;

use maviola::asnc::node::Event;
use maviola::prelude::*;

use crate::stats::LinkStats;

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
            stats: LinkStats::default(),
        }
    }
}
