use core::f32;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use maviola::asnc::node::Callback;
use maviola::core::io::{ChannelId, ChannelInfo};
use maviola::prelude::Frame;
use maviola::prelude::Message;
use maviola::prelude::V2;
use maviola::prelude::{CallbackApi, Endpoint};
use maviola::protocol::SystemId;
use mavspec::rust::dialects::common::enums::{
    MavCmd, MavFrame, MavModeFlag, MavParamType, MavStandardMode, MavType,
};
use mavspec::rust::dialects::common::messages::{
    Attitude, AvailableModes, BatteryStatus, CommandInt, CommandLong, GlobalPositionInt,
    HomePosition, LinkNodeStatus, ParamSet, PositionTargetGlobalInt, RadioStatus, ServoOutputRaw,
    VfrHud,
};
use mavspec::rust::dialects::common::messages::{Heartbeat, LocalPositionNed};

use db::{Db, DbError};
use mavspec::rust::dialects::Common;

use crate::protocols::params::ParamProgress;
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

    pub fn last_heartbeat(&self) -> Result<Option<Heartbeat>, DbError> {
        self.db.last_heartbeat_for_system((self.system_id, 0x1))
    }

    pub fn last_global_position_int(&self) -> Result<Option<GlobalPositionInt>, DbError> {
        self.db
            .last_global_position_int_for_system((self.system_id, 0x1))
    }

    pub fn last_local_position_ned(&self) -> Result<Option<LocalPositionNed>, DbError> {
        self.db
            .last_local_position_ned_for_system((self.system_id, 0x1))
    }

    pub fn last_attitude(&self) -> Result<Option<Attitude>, DbError> {
        self.db.last_attitude_for_system((self.system_id, 0x1))
    }

    pub fn last_vfr_hud(&self) -> Result<Option<VfrHud>, DbError> {
        self.db.last_vfr_hud_for_system((self.system_id, 0x1))
    }

    pub fn last_servo_output_raw(&self) -> Result<Option<ServoOutputRaw>, DbError> {
        self.db
            .last_servo_output_raw_for_system((self.system_id, 0x1))
    }

    // TODO: properly handle multiple batteries / different instance IDs
    pub fn last_battery_status(&self) -> Result<Option<BatteryStatus>, DbError> {
        self.db
            .last_battery_status_for_system((self.system_id, 0x1))
    }

    pub fn last_target_global_int(&self) -> Result<Option<PositionTargetGlobalInt>, DbError> {
        self.db
            .last_position_target_global_int_for_system((self.system_id, 0x1))
    }

    pub fn last_home_position(&self) -> Result<Option<HomePosition>, DbError> {
        self.db.last_home_position_for_system((self.system_id, 0x1))
    }

    pub fn last_radio_status_for_system(&self) -> Result<Option<RadioStatus>, DbError> {
        self.db.last_radio_status_for_system((self.system_id, 0x1))
    }

    pub fn last_link_node_status_for_system(&self) -> Result<Option<LinkNodeStatus>, DbError> {
        self.db
            .last_link_node_status_for_system((self.system_id, 0x1))
    }

    pub fn mav_type(&self) -> MavType {
        match self.last_heartbeat().unwrap_or(None).map(|hb| hb.type_) {
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
            tracing::error!(system_id = self.system_id, "Failed to send message: {e:?}")
        }
    }

    pub fn do_reposition(&self, lat: f64, lng: f64, altitude_msl: f32) {
        let cmd = CommandInt {
            target_system: self.system_id,
            target_component: 0x01,
            frame: MavFrame::GlobalTerrainAltInt,
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
            param1: (arm as u8) as f32,
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
            param1: (standard_mode as u16) as f32,
            ..Default::default()
        };

        self.send_message(&cmd);
    }

    pub fn request_can_forwarding(&self, enable: bool) {
        let cmd = CommandLong {
            target_system: self.system_id,
            target_component: 0x01,
            command: MavCmd::CanForward,
            param1: (enable as u8) as f32,
            ..Default::default()
        };

        self.send_message(&cmd);
    }

    pub fn set_param(&self, param_id: &str, param_type: MavParamType, param_value: f32) {
        let mut param_id_bytes = [0; 16];
        param_id_bytes.copy_from_slice(param_id.as_bytes());

        let cmd = ParamSet {
            target_system: self.system_id,
            target_component: 0x01,
            param_id: param_id_bytes,
            param_type,
            param_value,
        };

        self.send_message(&cmd);
    }

    pub fn notify_of_message(
        &mut self,
        message: Common,
        frame: &Frame<V2>,
        callback: &Callback<V2>,
        endpoint: Arc<Mutex<Endpoint<V2>>>,
    ) {
        let _ = self.message_sender.send(message);

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
        let heartbeat = self.last_heartbeat().ok().flatten()?;

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
