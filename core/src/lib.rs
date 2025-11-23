use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use socketcan::{CanAddr, CanSocket, EmbeddedFrame, ExtendedId, Id, Socket, StandardId};
use tokio::sync::mpsc::Receiver;

use maviola::asnc::prelude::*;
use maviola::prelude::*;
use maviola::protocol::SystemId;
use maviola::protocol::dialects::Ardupilotmega;
use maviola::protocol::dialects::Common;
use mavspec::rust::dialects::common::enums::MavSeverity;

use db::Db;

mod links;
mod protocols;
mod stats;
mod system;

pub use links::*;
pub use system::*;

pub const GROUND_STATION_SYSTEM_ID: u8 = 0xfe;
pub const GROUND_STATION_COMPONENT_ID: u8 = 1;

#[derive(Clone)]
pub struct Core {
    event_sender: tokio::sync::mpsc::Sender<(LinkId, Event<V2>)>,
    pub db: Db,
    pub systems: Arc<Mutex<HashMap<SystemId, System>>>,
    pub links: Arc<Mutex<HashMap<LinkId, Link>>>,
    pub plot_origin: chrono::DateTime<chrono::Utc>,
}

#[derive(Default)]
pub struct CoreBuilder {
    pub links: Vec<LinkId>,
    pub autoconnect_usb: bool,
    pub on_event: Option<Box<dyn Fn(&Event<V2>) + Send + Sync>>,
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

    pub fn on_event(mut self, cb: Box<dyn Fn(&Event<V2>) + Send + Sync>) -> Self {
        self.on_event = Some(cb);
        self
    }

    pub fn spawn(self) -> Core {
        let (tx, rx) = tokio::sync::mpsc::channel(32);

        let core = Core {
            event_sender: tx,
            plot_origin: chrono::Utc::now(),
            db: Db::init(),
            systems: Arc::new(Mutex::new(HashMap::new())),
            links: Arc::new(Mutex::new(HashMap::new())),
        };

        let c = core.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
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
        on_event: Option<Box<dyn Fn(&Event<V2>) + Send + Sync>>,
    ) {
        for id in initial_links {
            self.add_link(id);
        }

        if autoconnect_usb {
            tokio::spawn(links::usb::autoconnect(self.clone()));
        }

        let socket = CanAddr::from_iface("vcan0")
            .map(|addr| CanSocket::open_addr(&addr))
            .flatten();

        while let Some((link, event)) = event_receiver.recv().await {
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
                        self.links
                            .lock()
                            .unwrap()
                            .get_mut(&link)
                            .unwrap()
                            .stats
                            .push_received(frame.body_length());
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
        let mut system_ids: Vec<SystemId> = self.systems.lock().unwrap().keys().cloned().collect();
        system_ids.sort();
        system_ids.dedup();
        system_ids
    }

    pub fn system<'a>(&'a self, id: SystemId) -> Option<System> {
        self.systems.lock().unwrap().get(&id).map(|s| s.clone())
    }

    pub fn links(&self) -> Vec<Link> {
        self.links
            .lock()
            .unwrap()
            .values()
            .map(|l| l.clone())
            .collect()
    }
}
