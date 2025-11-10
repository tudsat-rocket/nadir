use std::sync::{Arc, Mutex};

mod interface;
mod system;

use db::Db;
use interface::*;
use system::*;

use maviola::asnc::prelude::*;
use maviola::prelude::*;
use maviola::protocol::Peer;
use maviola::protocol::SystemId;
use maviola::protocol::dialects::Ardupilotmega;
use maviola::protocol::dialects::Common;

pub const GROUND_STATION_SYSTEM_ID: u8 = 250;
pub const GROUND_STATION_COMPONENT_ID: u8 = 1;

#[derive(Clone)]
pub struct Core {
    pub plot_origin: chrono::DateTime<chrono::Utc>,
    pub db: Db,
    pub peers: Arc<Mutex<Vec<Peer>>>,
    pub interfaces: Arc<Mutex<Vec<Interface>>>,
}

impl Core {
    pub fn init() -> Self {
        Self {
            plot_origin: chrono::Utc::now(),
            db: Db::init(),
            peers: Arc::new(Mutex::new(vec![])),
            interfaces: Arc::new(Mutex::new(vec![
                Interface::TcpClient("127.0.0.1:5760".to_owned()),
                Interface::TcpClient("127.0.0.1:5761".to_owned()),
                Interface::TcpClient("127.0.0.1:5762".to_owned()),
            ])),
        }
    }

    pub async fn run(self) {
        let mut network =
            Network::asnc().retry(RetryStrategy::Always(std::time::Duration::from_millis(500)));
        for interface in self.interfaces.lock().unwrap().iter() {
            match interface {
                Interface::TcpClient(s) => {
                    network = network.add_connection(TcpClient::new(s).unwrap())
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

        let mut events = node.events().unwrap();
        while let Some(event) = events.next().await {
            match event {
                Event::Frame(frame, callback) => {
                    if let Ok(message) = frame.decode::<Common>() {
                        match &message {
                            Common::Statustext(inner) => {
                                tracing::warn!("{:?}", inner);
                            }
                            _ => {}
                        };

                        if let Err(e) = self.db.write_common_message(message, frame, callback) {
                            tracing::error!("Failed to process common message: {e:?}");
                        }
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
                    self.peers.lock().unwrap().push(peer);
                }
                Event::PeerLost(peer) => {
                    tracing::warn!("Peer Lost: {peer:?}");
                }
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

    pub fn system(&self, id: SystemId) -> System {
        System {
            system_id: id,
            db: self.db.clone(),
        }
    }
}
