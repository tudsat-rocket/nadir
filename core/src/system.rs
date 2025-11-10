use db::DbError;
use maviola::protocol::SystemId;

use db::Db;
// use mavspec::rust::dialects::common::enums::MavAutopilot;
// use mavspec::rust::dialects::common::enums::MavMode;
// use mavspec::rust::dialects::common::enums::MavProtocolCapability;
// use mavspec::rust::dialects::common::enums::MavState;
use mavspec::rust::dialects::common::enums::MavType;
use mavspec::rust::dialects::common::messages::GlobalPositionInt;
use mavspec::rust::dialects::common::messages::Heartbeat;

pub struct System {
    pub system_id: SystemId,
    pub db: Db,
}

// pub enum OurAutopilot {
//     Rapid,
//     Standard(MavAutopilot),
// }

// pub struct SystemInfo {
//     // HEARTBEAT data from standard dialect
//     mav_type: MavType,
//     autopilot: OurAutopilot,
//     base_mode: MavMode,
//     custom_mode: u32,
//     system_status: MavState,
//     mavlink_version: Option<MavLinkVersion>,
//     // GLOBAL_POSITION_INT data from standard dialect
//     time_boot_ms: Option<u32>,
//     latitude: Option<f64>,
//     longitude: Option<f64>,
//     altitude_msl: Option<f64>,
//     altitude_above_home: Option<f64>,
//     // AUTOPILOT_VERSION data from standard dialect
//     capabilities: Option<MavProtocolCapability>,
//     flight_software_version: Option<u32>,
//     middleware_software_version: Option<u32>,
//     os_software_version: Option<u32>,
//     board_version: Option<u32>,
//     flight_custom_version: Option<[u8; 8]>,
//     middleware_custom_version: Option<[u8; 8]>,
//     os_custom_version: Option<[u8; 8]>,
//     vendor_id: Option<u16>,
//     product_id: Option<u16>,
//     uid: Option<[u8; 18]>,
// }

impl System {
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
}
