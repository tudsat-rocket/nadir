use core::f32;
use std::{collections::HashMap, time::Duration};

use maviola::protocol::ComponentId;
use mavspec::rust::{
    default_dialect::messages::ParamValue,
    dialects::{
        Common,
        common::{
            enums::{MavParamType, MavResult},
            messages::{ParamRequestList, ParamRequestRead},
        },
    },
};
use tokio::time::sleep;

use crate::{
    System,
    protocols::{Gatherable, gather},
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
        use MavParamType::{Real32, Real64, Int8, Uint8, Int16, Uint16, Int32, Uint32, Int64, Uint64};
        use ParamEncoding::{Cast, Bytewise};

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
        use ParamEncoding::{Cast, Bytewise};

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
            (Bytewise, Self::Int16(i)) => (MavParamType::Int16, f32::from_bits(i32::from(i) as u32)),
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
    Failed(MavResult),
    Progress(usize, usize),
    Complete(HashMap<ParamId, Param>),
}

impl Gatherable for ParamValue {
    type InitialRequest = ParamRequestList;
    type SpecificRequest = ParamRequestRead;

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
) {
    // Wait for the first AUTOPILOT_VERSION message. We need the device capabilities to check for
    // the flags which tell us how the parameter values are encoded.
    let encoding = loop {
        if let Some(encoding) = system.parameter_encoding() {
            break encoding;
        }

        sleep(Duration::from_millis(500)).await;
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

    match result {
        Ok(params_vec) => {
            let map: HashMap<_, _> = params_vec
                .into_iter()
                .map(|p: ParamValue| {
                    let id = String::from_utf8_lossy(&p.param_id).to_string();
                    let trimmed = id.trim_matches('\0').to_string();

                    let value = ParamVal::decode(p.param_type, p.param_value, encoding);

                    let param = Param {
                        id: trimmed,
                        value,
                        downloaded_value: value,
                    };

                    (param.id.clone(), param)
                })
                .collect();

            *system.params.lock().unwrap() = ParamProgress::Complete(map);
        }
        Err(res) => *system.params.lock().unwrap() = ParamProgress::Failed(res),
    }

    // Now we maintain the list. If a change is made from the UI, the system responds with another
    // PARAM_VALUE message, so we listen for these from this task and keep our parameter storage
    // updated with the downloaded (saved to the system/vehicle) values.
    loop {
        if let Ok(Common::ParamValue(value)) = message_rx.recv().await {
            let id = String::from_utf8_lossy(&value.param_id)
                .trim_matches('\0')
                .to_string();
            let mut progress = system.params.lock().unwrap();
            let ParamProgress::Complete(params) = &mut *progress else {
                continue;
            };

            if let Some(param) = params.get_mut(&id) {
                let value = ParamVal::decode(value.param_type, value.param_value, encoding);
                param.downloaded_value = value;
            }
        }
    }
}
