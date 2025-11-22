use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use maviola::asnc::prelude::*;
use maviola::prelude::*;
use maviola::protocol::Peer;
use maviola::protocol::SystemId;
use maviola::protocol::dialects::Ardupilotmega;
use maviola::protocol::dialects::Common;
use mavspec::rust::dialects::common::enums::MavSeverity;
use mavspec::rust::dialects::common::messages::CommandAck;

use socketcan::{CanAddr, CanSocket, EmbeddedFrame, ExtendedId, Id, Socket, StandardId};

use db::Db;

mod discovery;
mod interface;
mod system;

use interface::*;
pub use system::*;

pub const GROUND_STATION_SYSTEM_ID: u8 = 0xfe;
pub const GROUND_STATION_COMPONENT_ID: u8 = 1;

#[derive(Clone)]
pub struct Core {
    pub plot_origin: chrono::DateTime<chrono::Utc>,
    pub db: Db,
    pub peers: Arc<Mutex<Vec<Peer>>>,
    pub interfaces: Arc<Mutex<Vec<Interface>>>,
    pub systems: Arc<Mutex<HashMap<SystemId, System>>>,
    pub on_ack: Arc<Option<Box<dyn Fn(&CommandAck) + Send + Sync>>>,
    pub on_event: Arc<Option<Box<dyn Fn(&Event<V2>) + Send + Sync>>>,
}

impl Core {
    pub fn init() -> Self {
        Self {
            plot_origin: chrono::Utc::now(),
            db: Db::init(),
            peers: Arc::new(Mutex::new(vec![])),
            systems: Arc::new(Mutex::new(HashMap::new())),
            interfaces: Arc::new(Mutex::new(vec![
                //Interface::TcpClient("127.0.0.1:5760".to_owned()),
                //Interface::TcpClient("127.0.0.1:5761".to_owned()),
                //Interface::TcpClient("127.0.0.1:5762".to_owned()),
                Interface::UdpServer("0.0.0.0:14550".to_owned()),
            ])),
            on_ack: Arc::new(None),
            on_event: Arc::new(None),
        }
    }

    pub fn on_ack(mut self, cb: Box<dyn Fn(&CommandAck) + Send + Sync>) -> Self {
        self.on_ack = Arc::new(Some(cb));
        self
    }

    pub fn on_event(mut self, cb: Box<dyn Fn(&Event<V2>) + Send + Sync>) -> Self {
        self.on_event = Arc::new(Some(cb));
        self
    }

    pub async fn run(self) {
        let mut network =
            Network::asnc().retry(RetryStrategy::Always(std::time::Duration::from_millis(500)));
        for interface in self.interfaces.lock().unwrap().iter() {
            match interface {
                Interface::TcpClient(s) => {
                    network = network.add_connection(TcpClient::new(s).unwrap())
                }
                Interface::UdpServer(s) => {
                    network = network.add_connection(UdpServer::new(s).unwrap())
                }
            }
        }

        let node = Node::asnc::<V2>()
            .id(MavLinkId::new(
                GROUND_STATION_SYSTEM_ID,
                GROUND_STATION_COMPONENT_ID,
            ))
            .connection(network)
            .build()
            .await
            .unwrap();

        let socket = CanAddr::from_iface("vcan0")
            .map(|addr| CanSocket::open_addr(&addr))
            .flatten();

        let mut events = node.events().unwrap();
        while let Some(event) = events.next().await {
            match &event {
                Event::Frame(frame, callback) => {
                    if let Ok(message) = frame.decode::<Common>() {
                        match &message {
                            Common::Statustext(inner) => match inner.severity {
                                MavSeverity::Debug => tracing::debug!(
                                    system_id = frame.system_id(),
                                    component_id = frame.component_id(),
                                    "{}",
                                    &String::from_utf8_lossy(&inner.text),
                                ),
                                MavSeverity::Info | MavSeverity::Notice => tracing::info!(
                                    system_id = frame.system_id(),
                                    component_id = frame.component_id(),
                                    "{}",
                                    &String::from_utf8_lossy(&inner.text),
                                ),
                                MavSeverity::Warning => tracing::warn!(
                                    system_id = frame.system_id(),
                                    component_id = frame.component_id(),
                                    "{}",
                                    &String::from_utf8_lossy(&inner.text),
                                ),
                                MavSeverity::Error
                                | MavSeverity::Alert
                                | MavSeverity::Critical
                                | MavSeverity::Emergency => tracing::error!(
                                    system_id = frame.system_id(),
                                    component_id = frame.component_id(),
                                    "{}",
                                    &String::from_utf8_lossy(&inner.text),
                                ),
                            },
                            Common::CommandAck(ack) => {
                                if let Some(cb) = self.on_ack.as_ref() {
                                    cb(ack);
                                }
                            }
                            Common::CanFrame(can_frame) => {
                                if let Ok(s) = &socket {
                                    let id = if can_frame.id > 0b111_1111_1111 {
                                        Id::Extended(ExtendedId::new(can_frame.id).unwrap())
                                    } else {
                                        Id::Standard(StandardId::new(can_frame.id as u16).unwrap())
                                    };

                                    let frame = socketcan::CanFrame::new(
                                        id,
                                        &can_frame.data[..(can_frame.len as usize)],
                                    )
                                    .unwrap();
                                    let _ = s.write_frame(&frame);
                                }
                            }
                            _ => {}
                        };

                        if let Err(e) = self.db.write_common_message(
                            message.clone(),
                            frame.clone(),
                            callback.clone(),
                        ) {
                            tracing::error!("Failed to process common message: {e:?}");
                        }

                        let system_id = frame.system_id();

                        let mut systems = self.systems.lock().unwrap();

                        let system = systems.entry(system_id).or_insert_with(|| {
                            System::new(system_id, self.db.clone(), callback.clone())
                        });

                        system.notify_of_message(message, frame, callback);
                    } else if let Ok(message) = frame.decode::<Ardupilotmega>() {
                        match message {
                            Ardupilotmega::Ahrs(_) => {}
                            Ardupilotmega::Ahrs2(_) => {}
                            Ardupilotmega::AoaSsa(_) => {}
                            Ardupilotmega::EkfStatusReport(_) => {}
                            Ardupilotmega::Meminfo(_) => {}
                            Ardupilotmega::EscTelemetry1To4(_) => {}
                            Ardupilotmega::Wind(_) => {}
                            Ardupilotmega::Simstate(_) => {}
                            msg => tracing::info!("{:?}", msg),
                        }
                    }
                }
                Event::Invalid(_frame, _frame_error, _callback) => {}
                Event::NewPeer(peer) => {
                    tracing::info!("New Peer: {peer:?}");
                    self.peers.lock().unwrap().push(peer.clone());
                }
                Event::PeerLost(peer) => {
                    tracing::warn!("Peer Lost: {peer:?}");
                }
            }

            if let Some(cb) = self.on_event.as_ref() {
                cb(&event);
            }
        }
    }

    pub fn known_system_ids(&self) -> Vec<SystemId> {
        let mut system_ids: Vec<SystemId> = self
            .peers
            .lock()
            .unwrap()
            .iter()
            .map(|p| p.system_id())
            .collect();
        system_ids.sort();
        system_ids.dedup();
        system_ids
    }

    pub fn system<'a>(&'a self, id: SystemId) -> Option<System> {
        self.systems.lock().unwrap().get(&id).map(|s| s.clone())
    }
}
