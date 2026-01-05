use core::f32;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use maviola::asnc::node::Callback;
use maviola::core::io::{ChannelId, ChannelInfo};
use maviola::prelude::Frame;
use maviola::prelude::Message;
use maviola::prelude::V2;
use maviola::prelude::{CallbackApi as _, Endpoint};
use maviola::protocol::SystemId;
use mavspec::rust::default_dialect::enums::MavProtocolCapability;
use mavspec::rust::dialects::common::enums::{
    MavAutopilot, MavCmd, MavFrame, MavModeFlag, MavStandardMode, MavType,
};
use mavspec::rust::dialects::common::messages::{
    AutopilotVersion, AvailableModes, CommandInt, CommandLong, Heartbeat, ParamSet,
};

use db::{Db, DbError, MessageExt};
use mavspec::rust::dialects::Common;

use crate::protocols::params::{ParamEncoding, ParamProgress, ParamVal};
use crate::stats::ChannelStats;

pub struct SystemConnection {
    pub callback: Callback<V2>,
    pub endpoint: Arc<Mutex<Endpoint<V2>>>,
    pub channels: HashMap<ChannelId, (ChannelInfo, ChannelStats)>,
}

#[derive(Clone)]
pub struct System {
    pub system_id: SystemId,
    pub db: Db,
    pub message_sender: tokio::sync::broadcast::Sender<Common>,
    pub conn: Arc<Mutex<SystemConnection>>,
    pub available_modes: Arc<Mutex<Option<Vec<AvailableModes>>>>,
    pub params: Arc<Mutex<ParamProgress>>,
}

impl System {
    pub fn new(
        system_id: SystemId,
        db: Db,
        callback: Callback<V2>,
        endpoint: Arc<Mutex<Endpoint<V2>>>,
    ) -> Self {
        let available_modes = Arc::new(Mutex::new(None));
        let params = Arc::new(Mutex::new(ParamProgress::Unknown));

        // TODO: dialects
        let (message_sender, receiver) = tokio::sync::broadcast::channel::<Common>(5);
        let receiver2 = message_sender.subscribe();
        let receiver3 = message_sender.subscribe();

        let system = System {
            system_id,
            db,
            message_sender,
            conn: Arc::new(Mutex::new(SystemConnection {
                callback,
                endpoint,
                channels: HashMap::new(),
            })),
            available_modes,
            params,
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

        system
    }

    pub fn last_message<M: MessageExt + Default>(&self) -> Result<M, DbError> {
        self.db.last_message(self.system_id, 0x01)
    }

    pub fn all_messages<M: MessageExt + Default>(
        &self,
    ) -> Result<Vec<(DateTime<Utc>, M)>, DbError> {
        self.db.all_messages(self.system_id, 0x01)
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

    pub fn send_message(&self, message: &dyn Message) {
        let mut connection = self.conn.lock().unwrap();

        let frame = {
            let endpoint = connection.endpoint.lock().unwrap();
            endpoint.next_frame(message).unwrap()
        };

        let channel_id = connection.callback.channel_id();
        if let Some((_, stats)) = connection.channels.get_mut(&channel_id) {
            stats.push_sent(frame.body_length());
        }

        if let Err(e) = connection.callback.respond(&frame) {
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
        callback: &Callback<V2>,
        endpoint: Arc<Mutex<Endpoint<V2>>>,
    ) {
        let _ = self.message_sender.send(message);
        self.notify_of_frame(frame, callback, endpoint);
    }

    pub fn notify_of_frame(
        &mut self,
        frame: &Frame<V2>,
        callback: &Callback<V2>,
        endpoint: Arc<Mutex<Endpoint<V2>>>,
    ) {
        let mut conninfo = self.conn.lock().unwrap();

        conninfo.callback = callback.clone();
        conninfo.endpoint = endpoint;
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
                String::from_utf8_lossy(&mode_info.mode_name).to_string()
            } else {
                format!("{:?}", mode_info.standard_mode)
            }
        })
    }
}
