//! One import surface for the `MAVLink` types, whichever stack is underneath.
//!
//! maviola cannot build for wasm: it turns on tokio's `net` and `fs` features and pulls
//! `tokio-serial` unconditionally. The wasm build takes the protocol types from mavio instead, and
//! stubs out the few that are maviola's own - all of them from the uplink half of the API, which a
//! viewer does not have.

#[cfg(not(target_arch = "wasm32"))]
pub use maviola::asnc::node::{Callback, Event};
#[cfg(not(target_arch = "wasm32"))]
pub use maviola::core::io::{ChannelDetails, ChannelId, ChannelInfo};
#[cfg(not(target_arch = "wasm32"))]
pub use maviola::error::FrameError;
#[cfg(not(target_arch = "wasm32"))]
pub use maviola::prelude::{CallbackApi, Endpoint, Frame, MavLinkId, Message, V2};
#[cfg(not(target_arch = "wasm32"))]
pub use maviola::protocol::{ComponentId, SystemId, dialects};

#[cfg(target_arch = "wasm32")]
pub use mavio::dialects;
#[cfg(target_arch = "wasm32")]
pub use mavio::error::FrameError;
#[cfg(target_arch = "wasm32")]
pub use mavio::prelude::{Endpoint, Frame, MavLinkId, Message, V2};
#[cfg(target_arch = "wasm32")]
pub use mavio::protocol::{ComponentId, SystemId};

#[cfg(target_arch = "wasm32")]
pub use wasm_uplink::{Callback, CallbackApi, ChannelDetails, ChannelId, ChannelInfo, Event, Peer};

/// Stand-ins for maviola's uplink types, so everything above the transport keeps its shape on a
/// target that has no transport.
#[cfg(target_arch = "wasm32")]
mod wasm_uplink {
    use core::marker::PhantomData;

    use mavio::prelude::{Frame, MaybeVersioned};

    pub type ChannelId = usize;

    #[derive(Clone, Debug, Default)]
    pub struct ChannelInfo;

    impl ChannelInfo {
        pub fn name(&self) -> &str {
            "remote"
        }

        pub fn details(&self) -> ChannelDetails {
            ChannelDetails::Other
        }
    }

    /// Mirrors the variants the links pane matches on. None of them occur here, but the pane still
    /// has to typecheck.
    #[derive(Clone, Debug)]
    pub enum ChannelDetails {
        TcpClient {
            server_addr: std::net::SocketAddr,
        },
        UdpServer {
            server_addr: std::net::SocketAddr,
            peer_addr: std::net::SocketAddr,
        },
        SerialPort {
            path: String,
            baud_rate: u32,
        },
        TcpServer {
            server_addr: std::net::SocketAddr,
            peer_addr: std::net::SocketAddr,
        },
        UdpClient {
            server_addr: std::net::SocketAddr,
            bind_addr: std::net::SocketAddr,
        },
        Other,
    }

    #[derive(Clone, Debug)]
    pub struct Peer {
        pub system_id: mavio::protocol::SystemId,
    }

    impl Peer {
        pub fn system_id(&self) -> mavio::protocol::SystemId {
            self.system_id
        }
    }

    /// A channel to answer a vehicle over. Never actually reachable here; see the module docs.
    #[derive(Clone, Debug)]
    pub struct Callback<V: MaybeVersioned> {
        info: ChannelInfo,
        _version: PhantomData<V>,
    }

    impl<V: MaybeVersioned> Callback<V> {
        pub fn channel_id(&self) -> ChannelId {
            0
        }

        pub fn info(&self) -> &ChannelInfo {
            &self.info
        }
    }

    pub trait CallbackApi<V: MaybeVersioned> {
        fn send(&self, frame: &Frame<V>) -> Result<(), UplinkUnavailable>;
        fn respond(&self, frame: &Frame<V>) -> Result<(), UplinkUnavailable>;
    }

    impl<V: MaybeVersioned> CallbackApi<V> for Callback<V> {
        fn send(&self, _frame: &Frame<V>) -> Result<(), UplinkUnavailable> {
            Err(UplinkUnavailable)
        }

        fn respond(&self, _frame: &Frame<V>) -> Result<(), UplinkUnavailable> {
            Err(UplinkUnavailable)
        }
    }

    #[derive(Debug, thiserror::Error)]
    #[error("this build has no uplink")]
    pub struct UplinkUnavailable;

    /// Only `Frame` is ever constructed - frames reach a viewer over a stream, not a maviola node.
    /// The rest keep the match arms above this layer compiling.
    #[derive(Clone, Debug)]
    pub enum Event<V: MaybeVersioned> {
        Frame(Frame<V>, Callback<V>),
        Invalid(Frame<V>, mavio::error::FrameError, Callback<V>),
        NewPeer(Peer),
        PeerLost(Peer),
    }
}
