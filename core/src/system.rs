use core::f32;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use maviola::asnc::node::Callback;
use maviola::prelude::CallbackApi;
use maviola::prelude::Frame;
use maviola::prelude::Message;
use maviola::prelude::V2;
use maviola::protocol::SystemId;
use mavspec::rust::dialects::common::enums::MavCmd;
use mavspec::rust::dialects::common::enums::MavFrame;
use mavspec::rust::dialects::common::enums::MavType;
use mavspec::rust::dialects::common::messages::Heartbeat;
use mavspec::rust::dialects::common::messages::{CanFrame, CommandInt};
use mavspec::rust::dialects::common::messages::{CommandLong, GlobalPositionInt};

use db::{Db, DbError};
use mavspec::rust::dialects::Common;

pub struct SystemConnection {
    pub seq: u8,
    pub callback: Callback<V2>,
}

#[derive(Clone)]
pub struct System {
    pub system_id: SystemId,
    pub db: Db,
    pub message_sender: tokio::sync::broadcast::Sender<Common>,
    pub conn: Arc<Mutex<SystemConnection>>,
    // TODO: replace with proper type, include all the mode flags and stuff
    pub custom_modes: Arc<Mutex<Option<HashMap<u32, String>>>>,
}

impl System {
    pub fn new(system_id: SystemId, db: Db, callback: Callback<V2>) -> Self {
        let custom_modes = Arc::new(Mutex::new(None));

        // TODO: dialects
        let (message_sender, receiver) = tokio::sync::broadcast::channel::<Common>(5);

        let system = System {
            system_id,
            db,
            message_sender,
            conn: Arc::new(Mutex::new(SystemConnection {
                seq: 0,
                callback: callback.clone(),
            })),
            custom_modes,
        };

        let _ = tokio::spawn(crate::discovery::discover_available_modes(
            system.clone(),
            0x01,
            receiver,
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

        tracing::info!("fn send_massage: callback");
        connection.callback.send(&frame).unwrap();
    }

    pub fn send_can_message(&self, can_frame: CanFrame) {
        self.send_message(&can_frame);
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

    pub fn notify_of_message(&mut self, message: Common) {
        let _ = self.message_sender.send(message);
    }

    pub fn custom_modes(&self) -> Option<HashMap<u32, String>> {
        self.custom_modes.lock().unwrap().clone()
    }
}
