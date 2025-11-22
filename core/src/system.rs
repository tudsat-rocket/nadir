use core::f32;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use maviola::asnc::node::Callback;
use maviola::core::io::{ChannelId, ChannelInfo};
use maviola::prelude::CallbackApi;
use maviola::prelude::Frame;
use maviola::prelude::Message;
use maviola::prelude::V2;
use maviola::protocol::SystemId;
use mavspec::rust::dialects::common::enums::{
    MavCmd, MavFrame, MavModeFlag, MavStandardMode, MavType,
};
use mavspec::rust::dialects::common::messages::{
    Attitude, AvailableModes, CommandInt, CommandLong, GlobalPositionInt,
};
use mavspec::rust::dialects::common::messages::{Heartbeat, LocalPositionNed};

use db::{Db, DbError};
use mavspec::rust::dialects::Common;

#[derive(Clone, Debug, Default)]
pub struct ChannelStats {
    last_1s: VecDeque<(Instant, u8, usize)>,
}

impl ChannelStats {
    pub fn packet_loss(&mut self) -> f32 {
        self.truncate_to_1s();

        let Some(mut p) = self.last_1s.front() else {
            return 0.0;
        };

        let mut missed: u64 = 0;
        let mut total: u64 = 0;
        for p2 in self.last_1s.iter().skip(1) {
            let diff = p2.1.wrapping_sub(p.1);
            missed += (diff - 1) as u64;
            total += diff as u64;
            p = p2;
        }

        (missed as f32) / (total as f32)
    }

    pub fn incoming_packet_rate(&mut self) -> f32 {
        self.truncate_to_1s();
        self.last_1s.iter().count() as f32
    }

    pub fn incoming_data_rate(&mut self) -> f32 {
        self.truncate_to_1s();
        self.last_1s.iter().map(|p| p.2).sum::<usize>() as f32
    }

    pub fn outgoing_packet_rate(&mut self) -> f32 {
        0.0
    }

    pub fn outgoing_data_rate(&mut self) -> f32 {
        0.0
    }

    fn truncate_to_1s(&mut self) {
        while self
            .last_1s
            .front()
            .map(|(t, ..)| t.elapsed().as_secs_f32() > 1.0)
            .unwrap_or(false)
        {
            let _ = self.last_1s.pop_front();
        }
    }

    pub fn push(&mut self, seq: u8, len: usize) {
        self.last_1s.push_back((Instant::now(), seq, len));
        self.truncate_to_1s();
    }
}

pub struct SystemConnection {
    pub seq: u8,
    pub callback: Callback<V2>,
    pub channels: HashMap<ChannelId, (ChannelInfo, ChannelStats)>,
}

#[derive(Clone)]
pub struct System {
    pub system_id: SystemId,
    pub db: Db,
    pub message_sender: tokio::sync::broadcast::Sender<Common>,
    pub conn: Arc<Mutex<SystemConnection>>,
    pub available_modes: Arc<Mutex<Option<Vec<AvailableModes>>>>,
}

impl System {
    pub fn new(system_id: SystemId, db: Db, callback: Callback<V2>) -> Self {
        let available_modes = Arc::new(Mutex::new(None));

        // TODO: dialects
        let (message_sender, receiver) = tokio::sync::broadcast::channel::<Common>(5);
        let receiver2 = message_sender.subscribe();

        let system = System {
            system_id,
            db,
            message_sender,
            conn: Arc::new(Mutex::new(SystemConnection {
                seq: 0,
                callback: callback.clone(),
                channels: HashMap::new(),
            })),
            available_modes,
        };

        let _ = tokio::spawn(crate::discovery::discover_available_modes(
            system.clone(),
            0x01,
            receiver,
        ));

        let _ = tokio::spawn(crate::discovery::request_message_intervals(
            system.clone(),
            receiver2,
        ));

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
            MavType::Gcs => "📡",
            _ => "?",
        }
    }

    pub fn send_message(&self, message: &dyn Message) {
        let mut connection = self.conn.lock().unwrap();
        connection.seq = connection.seq.wrapping_add(1);
        let frame = Frame::builder()
            .version(V2)
            .system_id(0xfe)
            .component_id(0x01)
            .sequence(connection.seq)
            .message(message)
            .unwrap()
            .build();

        connection.callback.send(&frame).unwrap();
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

    // TODO: consider standard modes. test with PX4
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

    // TODO: consider standard modes. test with PX4
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

    pub fn notify_of_message(
        &mut self,
        message: Common,
        frame: &Frame<V2>,
        callback: &Callback<V2>,
    ) {
        let _ = self.message_sender.send(message);

        let mut conninfo = self.conn.lock().unwrap();
        let (_channel_info, channel_stats) = conninfo
            .channels
            .entry(callback.channel_id())
            .or_insert_with(|| (callback.info().clone(), ChannelStats::default()));

        channel_stats.push(frame.sequence(), frame.body_length());
    }

    pub fn channels(&self) -> Vec<(ChannelInfo, ChannelStats)> {
        let conninfo = self.conn.lock().unwrap();
        conninfo.channels.values().cloned().collect()
    }

    pub fn available_modes(&self) -> Option<Vec<AvailableModes>> {
        self.available_modes.lock().unwrap().clone()
    }

    pub fn custom_mode_info(&self, custom_mode: u32) -> Option<AvailableModes> {
        self.available_modes()
            .map(|modes| {
                modes
                    .iter()
                    .find(|mode| {
                        //mode.standard_mode == MavStandardMode::NonStandard
                        mode.custom_mode == custom_mode
                    })
                    .cloned()
            })
            .flatten()
    }

    fn standard_mode_info(&self, standard_mode: MavStandardMode) -> Option<AvailableModes> {
        self.available_modes()
            .map(|modes| {
                modes
                    .iter()
                    .find(|mode| mode.standard_mode == standard_mode)
                    .cloned()
            })
            .flatten()
    }

    pub fn current_mode_info(&self) -> Option<AvailableModes> {
        let Some(heartbeat) = self.last_heartbeat().ok().flatten() else {
            return None;
        };

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
