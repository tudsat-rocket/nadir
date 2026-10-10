use std::f32;
use std::sync::Arc;
use std::{collections::HashMap, time::Duration};

use tokio::sync::Notify;

use crate::mav::ComponentId;
use crate::time::sleep;
use mavspec::rust::{
    default_dialect::messages::ParamValue,
    dialects::{
        Common,
        common::{
            enums::MavParamType,
            messages::{ParamRequestList, ParamRequestRead},
        },
    },
};

use crate::{
    System,
    protocols::{GatherError, Gatherable, gather},
};

#[derive(Clone, Copy, Debug)]
pub enum ParamEncoding {
    Bytewise,
    Cast,
}

pub type ParamId = String;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamVal {
    Int8(i8),
    Uint8(u8),
    Int16(i16),
    Uint16(u16),
    Int32(i32),
    Uint32(u32),
    Int64(i64),
    Uint64(u64),
    Float32(f32),
}

impl ParamVal {
    pub fn decode(param_type: MavParamType, raw: f32, encoding: ParamEncoding) -> Self {
        use MavParamType::{
            Int8, Int16, Int32, Int64, Real32, Real64, Uint8, Uint16, Uint32, Uint64,
        };
        use ParamEncoding::{Bytewise, Cast};

        match (encoding, param_type) {
            // Unclear why a "Real64" type even exists, considering the actual type of the raw
            // value is a 32-bit float.
            (_, Real32 | Real64) => Self::Float32(raw),
            (Cast, Int8) => Self::Int8(raw as i8),
            (Cast, Uint8) => Self::Uint8(raw as u8),
            (Cast, Int16) => Self::Int16(raw as i16),
            (Cast, Uint16) => Self::Uint16(raw as u16),
            (Cast, Int32) => Self::Int32(raw as i32),
            (Cast, Uint32) => Self::Uint32(raw as u32),
            (Cast, Int64) => Self::Int64(raw as i64),
            (Cast, Uint64) => Self::Uint64(raw as u64),
            (Bytewise, Int8) => Self::Int8((raw.to_bits() as i32) as i8),
            (Bytewise, Uint8) => Self::Uint8(raw.to_bits() as u8),
            (Bytewise, Int16) => Self::Int16((raw.to_bits() as i32) as i16),
            (Bytewise, Uint16) => Self::Uint16(raw.to_bits() as u16),
            (Bytewise, Int64) => Self::Int64(i64::from(raw.to_bits() as i32)),
            (Bytewise, Uint64) => Self::Uint64(u64::from(raw.to_bits())),
            (Bytewise, Int32) => Self::Int32(raw.to_bits() as i32),
            (Bytewise, Uint32) => Self::Uint32(raw.to_bits()),
        }
    }

    pub fn encode(self, encoding: ParamEncoding) -> (MavParamType, f32) {
        use ParamEncoding::{Bytewise, Cast};

        match (encoding, self) {
            (_, Self::Float32(f)) => (MavParamType::Real32, f),
            (Cast, Self::Int8(i)) => (MavParamType::Int8, f32::from(i)),
            (Cast, Self::Uint8(u)) => (MavParamType::Uint8, f32::from(u)),
            (Cast, Self::Int16(i)) => (MavParamType::Int16, f32::from(i)),
            (Cast, Self::Uint16(u)) => (MavParamType::Uint16, f32::from(u)),
            (Cast, Self::Int32(i)) => (MavParamType::Int32, i as f32),
            (Cast, Self::Uint32(u)) => (MavParamType::Uint32, u as f32),
            (Cast, Self::Int64(i)) => (MavParamType::Int64, i as f32),
            (Cast, Self::Uint64(u)) => (MavParamType::Uint64, u as f32),
            (Bytewise, Self::Int8(i)) => (MavParamType::Int8, f32::from_bits(i32::from(i) as u32)),
            (Bytewise, Self::Uint8(u)) => (MavParamType::Uint8, f32::from_bits(u32::from(u))),
            (Bytewise, Self::Int16(i)) => {
                (MavParamType::Int16, f32::from_bits(i32::from(i) as u32))
            }
            (Bytewise, Self::Uint16(u)) => (MavParamType::Uint16, f32::from_bits(u32::from(u))),
            (Bytewise, Self::Int32(i)) => (MavParamType::Int32, f32::from_bits(i as u32)),
            (Bytewise, Self::Uint32(u)) => (MavParamType::Uint32, f32::from_bits(u)),
            (Bytewise, Self::Int64(i)) => (MavParamType::Int64, f32::from_bits((i as i32) as u32)),
            (Bytewise, Self::Uint64(u)) => (MavParamType::Uint64, f32::from_bits(u as u32)),
        }
    }

    pub fn as_float(self) -> f32 {
        match self {
            Self::Float32(f) => f,
            _ => self.as_unsigned_int() as f32,
        }
    }

    pub fn as_unsigned_int(self) -> u64 {
        match self {
            Self::Int8(i) => i as u64,
            Self::Uint8(u) => u64::from(u),
            Self::Int16(i) => i as u64,
            Self::Uint16(u) => u64::from(u),
            Self::Int32(i) => i as u64,
            Self::Uint32(u) => u64::from(u),
            Self::Int64(i) => i as u64,
            Self::Uint64(u) => u,
            Self::Float32(f) => f as u64,
        }
    }
}

#[derive(Debug)]
pub struct Param {
    pub id: ParamId,
    pub value: ParamVal,
    pub downloaded_value: ParamVal,
}

// TODO: rename
pub enum ParamProgress {
    Unknown,
    Failed(GatherError),
    Progress(usize, usize),
    Complete(HashMap<ParamId, Param>),
}

impl Param {
    /// `PARAM_VALUE.param_id` is only NUL-terminated when shorter than 16 bytes.
    fn id_of(value: &ParamValue) -> ParamId {
        let end = value.param_id.iter().position(|&b| b == 0).unwrap_or(16);
        String::from_utf8_lossy(&value.param_id[..end]).into_owned()
    }
}

impl Gatherable for ParamValue {
    type InitialRequest = ParamRequestList;
    type SpecificRequest = ParamRequestRead;

    // ArduPilot queues 20 reads, zenith's uplink 32 frames shared with other traffic.
    const BATCH: usize = 16;

    fn index(&self) -> usize {
        self.param_index as usize
    }

    fn count(&self) -> usize {
        self.param_count as usize
    }

    fn unpack(msg: Common) -> Option<Self> {
        match msg {
            Common::ParamValue(inner) => Some(inner),
            _ => None,
        }
    }

    fn initial_request(system_id: u8, component_id: u8) -> Self::InitialRequest {
        ParamRequestList {
            target_system: system_id,
            target_component: component_id,
        }
    }

    fn specific_request(system_id: u8, component_id: u8, index: usize) -> Self::SpecificRequest {
        ParamRequestRead {
            target_system: system_id,
            target_component: component_id,
            param_id: [0x00; 16],
            param_index: index as i16,
        }
    }
}

pub async fn download_params(
    system: System,
    component_id: ComponentId,
    mut message_rx: tokio::sync::broadcast::Receiver<Common>,
    redownload: Arc<Notify>,
) {
    loop {
        // We need the device capabilities from AUTOPILOT_VERSION to know how parameter values are
        // encoded. Also waits out a mute, which would make every request vanish.
        let encoding = loop {
            if !system.muted()
                && let Some(encoding) = system.parameter_encoding()
            {
                break encoding;
            }

            sleep(Duration::from_millis(500)).await;
        };

        // Unsaved edits survive a redownload.
        let edits: HashMap<ParamId, ParamVal> = {
            let mut progress = system.params.lock().unwrap();
            let edits = match &*progress {
                ParamProgress::Complete(params) => params
                    .values()
                    .filter(|p| p.value != p.downloaded_value)
                    .map(|p| (p.id.clone(), p.value))
                    .collect(),
                _ => HashMap::new(),
            };
            *progress = ParamProgress::Unknown;
            edits
        };

        let params = system.params.clone();
        let result = gather(
            &system,
            component_id,
            &mut message_rx,
            Some(Box::new(move |received, total| {
                *params.lock().unwrap() = ParamProgress::Progress(received, total);
            })),
        )
        .await;

        *system.params.lock().unwrap() = match result {
            Ok(values) => ParamProgress::Complete(
                values
                    .iter()
                    .map(|p| {
                        let id = Param::id_of(p);
                        let downloaded_value =
                            ParamVal::decode(p.param_type, p.param_value, encoding);
                        let param = Param {
                            value: edits.get(&id).copied().unwrap_or(downloaded_value),
                            id: id.clone(),
                            downloaded_value,
                        };
                        (id, param)
                    })
                    .collect(),
            ),
            Err(e) => ParamProgress::Failed(e),
        };

        tokio::select! {
            () = redownload.notified() => {}
            () = track_changes(&system, &mut message_rx, encoding) => {}
        }
    }
}

/// Every write, ours or anyone else's, is answered with a `PARAM_VALUE`. Applies those to the
/// downloaded values, and to the shown value too unless the user is editing it.
async fn track_changes(
    system: &System,
    message_rx: &mut tokio::sync::broadcast::Receiver<Common>,
    encoding: ParamEncoding,
) {
    loop {
        let Ok(Common::ParamValue(value)) = message_rx.recv().await else {
            continue;
        };

        let mut progress = system.params.lock().unwrap();
        let ParamProgress::Complete(params) = &mut *progress else {
            continue;
        };

        if let Some(param) = params.get_mut(&Param::id_of(&value)) {
            let new = ParamVal::decode(value.param_type, value.param_value, encoding);
            if param.value == param.downloaded_value {
                param.value = new;
            }
            param.downloaded_value = new;
        }
    }
}
