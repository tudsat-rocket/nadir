use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;
use tokio::sync::mpsc::{self, Receiver};

use maviola::asnc::prelude::*;
use maviola::prelude::*;
use maviola::protocol::SystemId;
use maviola::protocol::dialects::Ardupilotmega;
use maviola::protocol::dialects::Common;
use mavspec::rust::dialects::common::enums::MavSeverity;

mod can_proxy;
mod links;
mod protocols;
mod stats;
mod system;

use db::Db;
pub use links::*;
pub use protocols::logs::types::*;
pub use protocols::params::{Param, ParamId, ParamProgress, ParamVal};
pub use system::*;

pub const GROUND_STATION_SYSTEM_ID: u8 = 0xfe;
pub const GROUND_STATION_COMPONENT_ID: u8 = 1;

pub type EventCallback = Box<dyn Fn(&Event<V2>) + Send + Sync>;

#[derive(Clone)]
pub struct Core {
    event_sender: tokio::sync::mpsc::Sender<(LinkId, Event<V2>)>,
    pub db: Db,
    pub systems: Arc<Mutex<HashMap<SystemId, System>>>,
    pub links: Arc<Mutex<HashMap<LinkId, Link>>>,
    pub plot_origin: chrono::DateTime<chrono::Utc>,
    pub can_proxy: Option<(
        mpsc::Sender<socketcan::CanFrame>,
        broadcast::Sender<socketcan::CanFrame>,
    )>,
}

#[derive(Default)]
pub struct CoreBuilder {
    pub links: Vec<LinkId>,
    pub autoconnect_usb: bool,
    pub on_event: Option<EventCallback>,
}

impl CoreBuilder {
    pub fn udp_server(mut self, addr: SocketAddr) -> Self {
        self.links.push(LinkId::UdpServer(addr));
        self
    }

    pub fn tcp_client(mut self, addr: SocketAddr) -> Self {
        self.links.push(LinkId::TcpClient(addr));
        self
    }

    pub fn autoconnect_to_usb(mut self) -> Self {
        self.autoconnect_usb = true;
        self
    }

    pub fn on_event(mut self, cb: EventCallback) -> Self {
        self.on_event = Some(cb);
        self
    }

    pub fn spawn(self) -> Core {
        let (tx, rx) = tokio::sync::mpsc::channel(32);

        // Clonable channel for sending CAN frames received via MAVLink to socketcan. Sender is
        // cloned for every connected system
        let (socketcan_tx_sender, socketcan_tx_receiver) =
            tokio::sync::mpsc::channel::<socketcan::CanFrame>(32);

        // Broadcast channel for sending CAN frames received via socketcan to connected MAVLink
        // systems. Each system can subscribe to this.
        let (socketcan_rx_publisher, _) =
            tokio::sync::broadcast::channel::<socketcan::CanFrame>(32);

        let core = Core {
            event_sender: tx,
            plot_origin: chrono::Utc::now(),
            db: Db::init(),
            systems: Arc::new(Mutex::new(HashMap::new())),
            links: Arc::new(Mutex::new(HashMap::new())),
            can_proxy: Some((socketcan_tx_sender, socketcan_rx_publisher.clone())),
        };

        let c = core.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();

            rt.spawn(async move {
                can_proxy::spawn_can_proxy(socketcan_tx_receiver, socketcan_rx_publisher);
            });

            rt.block_on(c.run(self.links, self.autoconnect_usb, rx, self.on_event));
        });

        core
    }
}

impl Core {
    pub fn builder() -> CoreBuilder {
        CoreBuilder::default()
    }

    pub fn add_link(&self, id: LinkId) {
        let link = id.spawn(self.event_sender.clone());
        self.links.lock().unwrap().insert(id, link);
    }

    pub(crate) async fn run(
        self,
        initial_links: Vec<LinkId>,
        autoconnect_usb: bool,
        mut event_receiver: Receiver<(LinkId, Event<V2>)>,
        on_event: Option<EventCallback>,
    ) {
        for id in initial_links {
            self.add_link(id);
        }

        if autoconnect_usb {
            tokio::spawn(links::usb::autoconnect(self.clone()));
        }

        while let Some((link_id, event)) = event_receiver.recv().await {
            match &event {
                Event::Frame(frame, callback) => {
                    if frame.system_id() == 0xff {
                        continue;
                    }

                    let mut links = self.links.lock().unwrap();
                    let link = links.get(&link_id).unwrap();

                    let mut systems = self.systems.lock().unwrap();
                    let system_id = frame.system_id();
                    let system = systems.entry(system_id).or_insert_with(|| {
                        System::new(
                            system_id,
                            self.db.clone(),
                            callback.clone(),
                            link.endpoint.clone(),
                            self.can_proxy.clone(),
                        )
                    });

                    if let Ok(message) = frame.decode::<Common>() {
                        // TODO: move this to its own protocol task as well to get it out of here
                        if let Common::Statustext(inner) = &message {
                            match inner.severity {
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
                            }
                        }

                        if let Err(e) =
                            self.db
                                .write_message(frame.system_id(), frame.component_id(), &message)
                        {
                            tracing::error!("Failed to process message: {e:?}");
                        }

                        system.notify_of_common_message(
                            message,
                            frame,
                            callback,
                            link.endpoint.clone(),
                        );
                    } else if let Ok(message) = frame.decode::<Ardupilotmega>() {
                        if let Err(e) =
                            self.db
                                .write_message(frame.system_id(), frame.component_id(), &message)
                        {
                            tracing::error!("Failed to process message: {e:?}");
                        }

                        system.notify_of_frame(frame, callback, link.endpoint.clone());
                    } else if let Ok(message) = frame.decode::<rapid_dialect::Rapid>() {
                        if let Err(e) =
                            self.db
                                .write_message(frame.system_id(), frame.component_id(), &message)
                        {
                            tracing::error!("Failed to process message: {e:?}");
                        }

                        let links = self.links.lock().unwrap();
                        let link = links.get(&link_id).unwrap();

                        let mut systems = self.systems.lock().unwrap();
                        let system_id = frame.system_id();
                        let Some(system) = systems.get_mut(&system_id) else {
                            continue;
                        };

                        system.notify_of_frame(frame, callback, link.endpoint.clone());
                    }

                    let link = links.get_mut(&link_id).unwrap();
                    link.stats.push_received(frame.body_length());
                }
                Event::Invalid(_frame, _frame_error, _callback) => {}
                Event::NewPeer(peer) => {
                    tracing::info!("New Peer: {peer:?}");
                }
                Event::PeerLost(peer) => {
                    tracing::warn!("Peer Lost: {peer:?}");
                }
            }

            if let Some(cb) = on_event.as_ref() {
                cb(&event);
            }
        }
    }

    pub fn known_system_ids(&self) -> Vec<SystemId> {
        let mut system_ids: Vec<SystemId> = self.systems.lock().unwrap().keys().copied().collect();
        system_ids.sort_unstable();
        system_ids.dedup();
        system_ids
    }

    pub fn system(&self, id: SystemId) -> Option<System> {
        self.systems.lock().unwrap().get(&id).cloned()
    }

    pub fn links(&self) -> Vec<Link> {
        self.links.lock().unwrap().values().cloned().collect()
    }
}
