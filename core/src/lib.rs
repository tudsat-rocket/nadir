#[cfg(not(target_arch = "wasm32"))]
mod can_proxy;
mod links;
pub mod mav;
mod protocols;
pub mod settings;
mod source;
mod stats;
mod system;
mod task;
mod time;
pub mod tlog;

pub use db::{MessageInstance, MessageSummary, format_message_label};
pub use links::*;
pub use protocols::logs::types::*;
pub use protocols::params::{Param, ParamId, ParamProgress, ParamVal};
pub use settings::Settings;
pub use source::*;
pub use system::*;

pub const GROUND_STATION_SYSTEM_ID: u8 = 0xfe;
pub const GROUND_STATION_COMPONENT_ID: u8 = 1;

/// Not a value the spec reserves, just the one `QGroundControl` and Mission Planner default to.
pub(crate) const OTHER_GROUND_STATION_SYSTEM_ID: u8 = 0xff;

/// The pair of channels bridging MAVLink-tunnelled CAN to a local socketcan interface.
///
/// Uninhabited off native, so `Option<CanProxy>` can only be `None` there.
#[cfg(not(target_arch = "wasm32"))]
pub type CanProxy = (
    tokio::sync::mpsc::Sender<socketcan::CanFrame>,
    tokio::sync::broadcast::Sender<socketcan::CanFrame>,
);
#[cfg(target_arch = "wasm32")]
pub type CanProxy = core::convert::Infallible;

/// Native only: a wasm build has no links to own, and reaches its telemetry through a [`Source`]
/// fed from a stream instead.
#[cfg(not(target_arch = "wasm32"))]
mod core_impl {
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

    use tokio::sync::mpsc::Receiver;

    use crate::mav::{Event, V2};
    use crate::source::Source;
    use crate::{Link, LinkId, OTHER_GROUND_STATION_SYSTEM_ID, can_proxy, links};

    pub type EventCallback = Box<dyn Fn(&Event<V2>) + Send + Sync>;

    /// Owns the links and the systems reachable over them.
    ///
    /// There is exactly one, for the lifetime of the process.
    #[derive(Clone)]
    pub struct Core {
        event_sender: tokio::sync::mpsc::Sender<(LinkId, Event<V2>)>,
        pub links: Arc<Mutex<HashMap<LinkId, Link>>>,
        pub live: Source,
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

        pub fn link(mut self, id: LinkId) -> Self {
            self.links.push(id);
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
                links: Arc::new(Mutex::new(HashMap::new())),
                live: Source::live(Some((socketcan_tx_sender, socketcan_rx_publisher.clone()))),
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
                let event_system_id = match &event {
                    Event::Frame(frame, _) | Event::Invalid(frame, _, _) => frame.system_id(),
                    Event::NewPeer(peer) | Event::PeerLost(peer) => peer.system_id(),
                };

                if event_system_id == OTHER_GROUND_STATION_SYSTEM_ID {
                    continue;
                }

                match &event {
                    Event::Frame(frame, callback) => {
                        // Logged before anything tries to decode it, so that a frame we cannot make
                        // sense of is still in the record.
                        if let Some(tlog) = &self.live.tlog {
                            tlog.log(frame.system_id(), frame);
                        }

                        self.live.ingest(frame, chrono::Utc::now(), Some(callback));

                        let mut links = self.links.lock().unwrap();
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

        pub fn links(&self) -> Vec<Link> {
            self.links.lock().unwrap().values().cloned().collect()
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use core_impl::{Core, CoreBuilder, EventCallback};
