use std::collections::HashMap;
use std::f32;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use tokio::sync::{broadcast, mpsc};

use crate::mav::{
    Callback, CallbackApi as _, ChannelId, ChannelInfo, Endpoint, Frame, MavLinkId, Message,
    SystemId, V2,
};
use mavspec::rust::default_dialect::enums::MavProtocolCapability;
use mavspec::rust::dialects::common::enums::{
    MavAutopilot, MavCmd, MavFrame, MavModeFlag, MavStandardMode, MavType,
};
use mavspec::rust::dialects::common::messages::{
    AutopilotVersion, AvailableModes, CommandInt, CommandLong, Heartbeat, ParamSet,
};
use rapid_dialect::rapid::enums::ValveId;

use db::{Db, DbError, MessageExt};
use mavspec::rust::dialects::Common;

use crate::protocols::logs::{FlightLogUiState, LogDlCommand};
use crate::protocols::params::{ParamEncoding, ParamProgress, ParamVal};
use crate::source::Origin;
use crate::stats::ChannelStats;
use crate::{GROUND_STATION_COMPONENT_ID, GROUND_STATION_SYSTEM_ID};

pub struct SystemConnection {
    /// The channel this system was last heard on, and the only one we answer it over. Absent for a
    /// system that arrived from a recording, which cannot be talked to at all.
    pub callback: Option<Callback<V2>>,
    pub endpoint: Arc<Mutex<Endpoint<V2>>>,
    pub channels: HashMap<ChannelId, (ChannelInfo, ChannelStats)>,
}

#[derive(Clone)]
pub struct System {
    pub system_id: SystemId,
    pub db: Db,
    pub tlog: Option<crate::tlog::Writer>,
    pub origin: Origin,
    pub message_sender: broadcast::Sender<Common>,
    pub conn: Arc<Mutex<SystemConnection>>,
    pub available_modes: Arc<Mutex<Option<Vec<AvailableModes>>>>,
    pub params: Arc<Mutex<ParamProgress>>,
    pub logs: Arc<Mutex<FlightLogUiState>>,
    pub log_cmd_tx: Arc<Mutex<mpsc::Sender<LogDlCommand>>>,
}

impl System {
    // `can_proxy` is uninhabited off native, where the bridge tasks below are compiled out.
    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    pub fn new(
        system_id: SystemId,
        db: Db,
        tlog: Option<crate::tlog::Writer>,
        origin: Origin,
        callback: Option<Callback<V2>>,
        can_proxy: Option<crate::CanProxy>,
    ) -> Self {
        let available_modes = Arc::new(Mutex::new(None));
        let params = Arc::new(Mutex::new(ParamProgress::Unknown));
        let logs = Arc::new(Mutex::new(FlightLogUiState::default()));

        // TODO: dialects
        let (message_sender, receiver) = tokio::sync::broadcast::channel::<Common>(512);
        let receiver2 = message_sender.subscribe();
        let receiver3 = message_sender.subscribe();
        let receiver_logs = message_sender.subscribe();

        let (log_cmd_tx, log_cmd_rx) = tokio::sync::mpsc::channel::<LogDlCommand>(1);

        // Each system gets its own endpoint (and thus its own MAVLink sequence counter)
        let endpoint = Arc::new(Mutex::new(Endpoint::new(MavLinkId {
            system: GROUND_STATION_SYSTEM_ID,
            component: GROUND_STATION_COMPONENT_ID,
        })));

        let system = System {
            system_id,
            db,
            tlog,
            origin,
            message_sender: message_sender.clone(),
            conn: Arc::new(Mutex::new(SystemConnection {
                callback,
                endpoint,
                channels: HashMap::new(),
            })),
            available_modes,
            params,
            logs,
            log_cmd_tx: Arc::new(Mutex::new(log_cmd_tx)),
        };

        std::mem::drop(tokio::spawn(crate::protocols::heartbeat::send_heartbeats(
            system.clone(),
        )));

        std::mem::drop(tokio::spawn(
            crate::protocols::modes::discover_available_modes(system.clone(), 0x01, receiver),
        ));

        std::mem::drop(tokio::spawn(
            crate::protocols::intervals::request_message_intervals(system.clone(), receiver2),
        ));

        std::mem::drop(tokio::spawn(crate::protocols::params::download_params(
            system.clone(),
            0x01,
            receiver3,
        )));

        std::mem::drop(tokio::spawn(crate::protocols::logs::run_log_worker(
            system.clone(),
            0x01,
            receiver_logs,
            log_cmd_rx,
        )));

        #[cfg(not(target_arch = "wasm32"))]
        if let Some((tx_sender, rx_publisher)) = can_proxy {
            std::mem::drop(tokio::spawn(crate::protocols::can::forward_to_socketcan(
                message_sender.subscribe(),
                tx_sender,
            )));

            std::mem::drop(tokio::spawn(crate::protocols::can::subscribe_to_socketcan(
                system.clone(),
                rx_publisher.subscribe(),
            )));
        }

        system
    }

    /// The latest moment this system knows about: see [`Origin::now`].
    pub fn now(&self) -> DateTime<Utc> {
        self.origin.now()
    }

    pub fn last_message<M: MessageExt + Default>(&self) -> Result<M, DbError> {
        self.db.last_message(self.system_id, 0x01)
    }

    pub fn last_instance_message<M: MessageExt + Default>(&self, id: i64) -> Result<M, DbError> {
        let instance = M::instance_field().map(|field| (field, id));
        self.db
            .last_message_filtered(self.system_id, 0x01, instance)
    }

    pub fn all_messages<M: MessageExt + Default>(&self) -> Vec<(DateTime<Utc>, M)> {
        self.db.all_messages(self.system_id, 0x01)
    }

    pub fn messages_since<M: MessageExt + Default>(
        &self,
        since: Option<DateTime<Utc>>,
        limit: Option<usize>,
    ) -> Vec<(DateTime<Utc>, M)> {
        self.db.messages_since(self.system_id, 0x01, since, limit)
    }

    pub fn message_count<M: MessageExt + Default>(&self) -> usize {
        self.db.message_count::<M>(self.system_id, 0x01)
    }

    #[deprecated]
    pub fn last_heartbeat(&self) -> Result<Option<Heartbeat>, DbError> {
        // TODO: cache last heartbeat, don't go through the database for this.
        self.last_message().map(Some)
    }

    pub fn mav_type(&self) -> MavType {
        match self.last_message::<Heartbeat>().ok().map(|hb| hb.type_) {
            Some(mt) => mt,
            None => MavType::Generic,
        }
    }

    pub fn icon(&self) -> &'static str {
        match self.mav_type() {
            MavType::Rocket => "🚀",
            MavType::Tricopter => "🚁",
            MavType::Quadrotor => "🚁",
            MavType::Hexarotor => "🚁",
            MavType::Octorotor => "🚁",
            MavType::Helicopter => "🚁",
            MavType::Coaxial => "🚁",
            MavType::FixedWing => "✈",
            MavType::GroundRover => "🚗",
            MavType::Gcs | MavType::AntennaTracker => "📡",
            _ => "?",
        }
    }

    pub fn parameter_encoding(&self) -> Option<ParamEncoding> {
        let heartbeat = self.last_message::<Heartbeat>().ok()?;
        let av = self.last_message::<AutopilotVersion>().ok()?;

        match (heartbeat.autopilot, av.capabilities) {
            (MavAutopilot::Ardupilotmega, _) => Some(ParamEncoding::Cast),
            (_, cap) if cap.contains(MavProtocolCapability::PARAM_ENCODE_BYTEWISE) => {
                Some(ParamEncoding::Bytewise)
            }
            (_, cap) if cap.contains(MavProtocolCapability::PARAM_ENCODE_C_CAST) => {
                Some(ParamEncoding::Cast)
            }
            _ => None,
        }
    }

    pub fn send_message<M: Message + MessageExt + Debug>(&self, message: &M) {
        let mut connection = self.conn.lock().unwrap();
        let SystemConnection {
            callback,
            endpoint,
            channels,
        } = &mut *connection;

        // No channel to answer on, which is what keeps a replay off a real link.
        let Some(callback) = callback.as_ref() else {
            tracing::debug!(
                system_id = self.system_id,
                "Discarding {message:?}, this system is read-only"
            );
            return;
        };

        let frame = {
            let endpoint = endpoint.lock().unwrap();
            endpoint.next_frame(message).unwrap()
        };

        // The log of the system we are talking to, not one of our own, see `Writer::log`.
        if let Some(tlog) = &self.tlog {
            tlog.log(self.system_id, &frame);
        }

        if let Some((_, stats)) = channels.get_mut(&callback.channel_id()) {
            stats.push_sent(frame.body_length());
        }

        // TODO: build a better way to track sent commands
        if message.id() == 75 || message.id() == 76 {
            self.db.write_message(self.system_id, 0x01, message);
        }

        if let Err(e) = callback.respond(&frame) {
            tracing::error!(system_id = self.system_id, "Failed to send message: {e:?}");
        }
    }

    pub fn do_reposition(&self, lat: f64, lng: f64, altitude_msl: f32) {
        let cmd = CommandInt {
            target_system: self.system_id,
            target_component: 0x01,
            frame: MavFrame::GlobalRelativeAltInt,
            command: MavCmd::DoReposition,
            current: 0,
            autocontinue: 0,
            param1: -1.0,
            param2: 0.0,
            param3: f32::NAN,
            param4: f32::NAN,
            x: (lat * 10_000_000.0) as i32,
            y: (lng * 10_000_000.0) as i32,
            z: altitude_msl,
        };

        self.send_message(&cmd);
    }

    pub fn do_arm(&self, arm: bool, force: bool) {
        let cmd = CommandLong {
            target_system: self.system_id,
            target_component: 0x01,
            command: MavCmd::ComponentArmDisarm,
            param1: f32::from(u8::from(arm)),
            param2: (if force { 21196 } else { 0 }) as f32,
            ..Default::default()
        };

        self.send_message(&cmd);
    }

    pub fn do_set_custom_mode(&self, custom_mode: u32) {
        let cmd = CommandLong {
            target_system: self.system_id,
            target_component: 0x01,
            command: MavCmd::DoSetMode,
            param1: 0x01 as f32,
            param2: custom_mode as f32,
            ..Default::default()
        };

        self.send_message(&cmd);
    }

    pub fn do_set_standard_mode(&self, standard_mode: MavStandardMode) {
        let cmd = CommandLong {
            target_system: self.system_id,
            target_component: 0x01,
            command: MavCmd::DoSetStandardMode,
            param1: f32::from(standard_mode as u16),
            ..Default::default()
        };

        self.send_message(&cmd);
    }

    pub fn do_set_valve(&self, valve: ValveId, state: f32) {
        let cmd = rapid_dialect::rapid::messages::CommandLong {
            target_system: self.system_id,
            target_component: 0x01,
            command: rapid_dialect::rapid::enums::MavCmd::CommandValve,
            param1: f32::from(valve.value()),
            param2: state,
            ..Default::default()
        };

        self.send_message(&cmd);
    }

    pub fn do_pulse_valve(&self, valve: ValveId, duration_secs: f32) {
        let cmd = rapid_dialect::rapid::messages::CommandLong {
            target_system: self.system_id,
            target_component: 0x01,
            command: rapid_dialect::rapid::enums::MavCmd::CommandValve,
            param1: f32::from(valve.value()),
            param2: 1.0, // 1.0 -> PulseOpen
            param3: duration_secs,
            ..Default::default()
        };

        self.send_message(&cmd);
    }

    pub fn request_can_forwarding(&self, enable: bool) {
        let cmd = CommandLong {
            target_system: self.system_id,
            target_component: 0x01,
            command: MavCmd::CanForward,
            param1: f32::from(u8::from(enable)),
            ..Default::default()
        };

        self.send_message(&cmd);
    }

    pub fn set_param(&self, param_id: &str, value: ParamVal) {
        let Some(encoding) = self.parameter_encoding() else {
            return; // TODO
        };

        let p_id_b = param_id.as_bytes();
        let mut param_id_bytes = [0; 16];
        param_id_bytes[..p_id_b.len()].copy_from_slice(p_id_b);

        let (param_type, param_value) = value.encode(encoding);

        let cmd = ParamSet {
            target_system: self.system_id,
            target_component: 0x01,
            param_id: param_id_bytes,
            param_type,
            param_value,
        };

        self.send_message(&cmd);
    }

    pub fn notify_of_common_message(
        &mut self,
        message: Common,
        frame: &Frame<V2>,
        callback: Option<&Callback<V2>>,
    ) {
        let _ = self.message_sender.send(message);
        self.notify_of_frame(frame, callback);
    }

    /// Records that a frame arrived, and over which channel to answer it.
    pub fn notify_of_frame(&mut self, frame: &Frame<V2>, callback: Option<&Callback<V2>>) {
        let Some(callback) = callback else {
            return;
        };

        let mut conninfo = self.conn.lock().unwrap();

        conninfo.callback = Some(callback.clone());
        let (_channel_info, channel_stats) = conninfo
            .channels
            .entry(callback.channel_id())
            .or_insert_with(|| (callback.info().clone(), ChannelStats::default()));

        channel_stats.push_received(frame.sequence(), frame.body_length());
    }

    pub fn channels(&self) -> Vec<(ChannelInfo, ChannelStats)> {
        let conninfo = self.conn.lock().unwrap();
        conninfo.channels.values().cloned().collect()
    }

    pub fn available_modes(&self) -> Option<Vec<AvailableModes>> {
        self.available_modes.lock().unwrap().clone()
    }

    pub fn custom_mode_info(&self, custom_mode: u32) -> Option<AvailableModes> {
        self.available_modes().and_then(|modes| {
            modes
                .iter()
                .find(|mode| {
                    //mode.standard_mode == MavStandardMode::NonStandard
                    mode.custom_mode == custom_mode
                })
                .cloned()
        })
    }

    pub fn current_mode_info(&self) -> Option<AvailableModes> {
        let heartbeat = self.last_message::<Heartbeat>().ok()?;

        if heartbeat
            .base_mode
            .contains(MavModeFlag::CUSTOM_MODE_ENABLED)
        {
            self.custom_mode_info(heartbeat.custom_mode)
        } else {
            // TODO: explain
            self.custom_mode_info(heartbeat.custom_mode)
        }
    }

    // TODO: refactor, replace available modes with our own type, implement these on there.
    pub fn current_mode_name(&self) -> Option<String> {
        self.current_mode_info().map(|mode_info| {
            if mode_info.standard_mode == MavStandardMode::NonStandard {
                mode_name_string(&mode_info.mode_name)
            } else {
                format!("{:?}", mode_info.standard_mode)
            }
        })
    }
}

// AVAILABLE_MODES.mode_name is `char[35]` and must be NUL-terminated per the MAVLink spec, but
// at least ArduPlane has been observed leaving uninitialised bytes past the name. Trim at the
// first NUL (or the buffer end) before lossy-decoding.
pub fn mode_name_string(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}
