use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use crate::stats::LinkStats;

#[cfg(not(target_arch = "wasm32"))]
pub mod tcp;
#[cfg(not(target_arch = "wasm32"))]
pub mod udp;
#[cfg(not(target_arch = "wasm32"))]
pub mod usb;

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "addr", rename_all = "snake_case")]
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

#[cfg(not(target_arch = "wasm32"))]
impl LinkId {
    pub fn spawn(
        &self,
        sender: tokio::sync::mpsc::Sender<(LinkId, crate::mav::Event<crate::mav::V2>)>,
    ) -> Link {
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
