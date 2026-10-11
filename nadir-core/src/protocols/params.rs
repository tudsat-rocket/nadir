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
    System, mode_name_string,
    protocols::{GatherError, Gatherable, gather},
};
use rapid_dialect::rapid::enums::ParamMetadataFlag;

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
            Self::Int8(i) => f32::from(i),
            Self::Int16(i) => f32::from(i),
            Self::Int32(i) => i as f32,
            Self::Int64(i) => i as f32,
            Self::Float32(f) => f,
            _ => self.as_unsigned_int() as f32,
        }
    }

    fn is_type_extreme(self) -> bool {
        match self {
            Self::Int8(i) => i == i8::MIN || i == i8::MAX,
            Self::Uint8(u) => u == u8::MIN || u == u8::MAX,
            Self::Int16(i) => i == i16::MIN || i == i16::MAX,
            Self::Uint16(u) => u == u16::MIN || u == u16::MAX,
            Self::Int32(i) => i == i32::MIN || i == i32::MAX,
            Self::Uint32(u) => u == u32::MIN || u == u32::MAX,
            Self::Int64(i) => i == i64::MIN || i == i64::MAX,
            Self::Uint64(u) => u == u64::MIN || u == u64::MAX,
            Self::Float32(f) => f.is_infinite() || f == f32::MIN || f == f32::MAX,
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

impl std::fmt::Display for ParamVal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int8(v) => v.fmt(f),
            Self::Uint8(v) => v.fmt(f),
            Self::Int16(v) => v.fmt(f),
            Self::Uint16(v) => v.fmt(f),
            Self::Int32(v) => v.fmt(f),
            Self::Uint32(v) => v.fmt(f),
            Self::Int64(v) => v.fmt(f),
            Self::Uint64(v) => v.fmt(f),
            Self::Float32(v) => v.fmt(f),
        }
    }
}

/// A value zenith splits over several consecutive `u32` params, `<base>_0`, `<base>_1`, ..., which
/// is shown and edited as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Composite {
    /// The `LoRa` frequency hopping seed (`*_BIND_0..1`), low word first. Derived from a binding
    /// phrase, which is never stored.
    HopSeed,
    /// The `LoRa` HMAC key (`*_KEY_0..3`), little-endian words. A vehicle reads it back as zeros
    /// over `LoRa`.
    Key,
}

impl Composite {
    /// The composite a param belongs to, with its base name and slot.
    pub fn of(id: &str) -> Option<(Self, &str, usize)> {
        let (base, slot) = id.rsplit_once('_')?;
        let slot: usize = slot.parse().ok()?;
        let composite = if base.ends_with("_BIND") {
            Self::HopSeed
        } else if base.ends_with("_KEY") {
            Self::Key
        } else {
            return None;
        };
        (slot < composite.width()).then_some((composite, base, slot))
    }

    pub fn width(self) -> usize {
        match self {
            Self::HopSeed => 2,
            Self::Key => 4,
        }
    }

    pub fn member(base: &str, slot: usize) -> ParamId {
        format!("{base}_{slot}")
    }

    /// The seed zenith hops with for a binding phrase: SipHash-2-4 with a zero key.
    pub fn hop_seed(phrase: &str) -> u64 {
        use std::hash::Hasher as _;

        let mut hasher = siphasher::sip::SipHasher::new_with_key(&[0; 16]);
        hasher.write(phrase.as_bytes());
        hasher.finish()
    }

    pub fn seed_words(seed: u64) -> [u32; 2] {
        [seed as u32, (seed >> 32) as u32]
    }

    pub fn seed_from_words([low, high]: [u32; 2]) -> u64 {
        u64::from(high) << 32 | u64::from(low)
    }

    /// The key as 32 hex digits, in the byte order it is used in.
    pub fn key_hex(words: [u32; 4]) -> String {
        words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    pub fn key_from_hex(hex: &str) -> Option<[u32; 4]> {
        let hex = hex.trim();
        if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let mut words = [0; 4];
        for (word, digits) in words.iter_mut().zip(hex.as_bytes().chunks(8)) {
            let digits = std::str::from_utf8(digits).ok()?;
            *word = u32::from_str_radix(digits, 16).ok()?.swap_bytes();
        }
        Some(words)
    }
}

#[derive(Debug)]
pub struct Param {
    pub id: ParamId,
    pub value: ParamVal,
    pub downloaded_value: ParamVal,
}

/// A param's static description from zenith's `PARAM_METADATA`. Unbounded params report the
/// extremes of their type as `min` and `max`.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamMetadata {
    pub default: ParamVal,
    pub min: ParamVal,
    pub max: ParamVal,
    pub units: Option<String>,
    pub description: String,
    pub reboot_required: bool,
}

impl ParamMetadata {
    pub fn decode(
        message: &rapid_dialect::rapid::messages::ParamMetadata,
        encoding: ParamEncoding,
    ) -> Option<(ParamId, Self)> {
        let param_type = MavParamType::try_from(message.param_type as u8).ok()?;
        let units = mode_name_string(&message.units);
        let metadata = Self {
            default: ParamVal::decode(param_type, message.default_value, encoding),
            min: ParamVal::decode(param_type, message.min_value, encoding),
            max: ParamVal::decode(param_type, message.max_value, encoding),
            units: (!units.is_empty()).then_some(units),
            description: mode_name_string(&message.description),
            reboot_required: message.flags.contains(ParamMetadataFlag::REBOOT_REQUIRED),
        };
        Some((mode_name_string(&message.param_id), metadata))
    }

    /// The lower bound, unless it is only the type's.
    pub fn min_bound(&self) -> Option<ParamVal> {
        (!self.min.is_type_extreme()).then_some(self.min)
    }

    /// The upper bound, unless it is only the type's.
    pub fn max_bound(&self) -> Option<ParamVal> {
        (!self.max.is_type_extreme()).then_some(self.max)
    }
}

// TODO: rename
pub enum ParamProgress {
    Unknown,
    Failed(GatherError),
    Progress(usize, usize),
    Complete(HashMap<ParamId, Param>),
}

impl Param {
    fn id_of(value: &ParamValue) -> ParamId {
        mode_name_string(&value.param_id)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The same vector zenith pins its derivation with.
    #[test]
    fn the_hop_seed_matches_the_vehicles() {
        assert_eq!(Composite::hop_seed("schinken"), 0x53bd_d841_8658_7bcb);
        let words = Composite::seed_words(0x53bd_d841_8658_7bcb);
        assert_eq!(words, [0x8658_7bcb, 0x53bd_d841]);
        assert_eq!(Composite::seed_from_words(words), 0x53bd_d841_8658_7bcb);
    }

    #[test]
    fn the_key_hex_is_in_byte_order() {
        let words = [0x0302_0100, 0x0706_0504, 0x0b0a_0908, 0x0f0e_0d0c];
        let hex = Composite::key_hex(words);
        assert_eq!(hex, "000102030405060708090a0b0c0d0e0f");
        assert_eq!(Composite::key_from_hex(&hex), Some(words));
        assert_eq!(Composite::key_from_hex("00"), None);
        assert_eq!(Composite::key_from_hex(&"g".repeat(32)), None);
        assert_eq!(
            Composite::key_from_hex(&format!("+{}", "0".repeat(31))),
            None
        );
    }

    #[test]
    fn metadata_decodes_like_param_values() {
        use rapid_dialect::rapid::enums::MavParamType as RapidParamType;

        let mut message = rapid_dialect::rapid::messages::ParamMetadata {
            param_id: *b"LORA_TX_PWR_HI\0\0",
            param_type: RapidParamType::Uint32,
            default_value: f32::from_bits(1500),
            min_value: f32::from_bits(0),
            max_value: f32::from_bits(u32::MAX),
            flags: ParamMetadataFlag::REBOOT_REQUIRED,
            units: [0; 16],
            description: [0; 200],
        };
        message.description[..5].copy_from_slice(b"Power");

        let (id, metadata) = ParamMetadata::decode(&message, ParamEncoding::Bytewise).unwrap();
        assert_eq!(id, "LORA_TX_PWR_HI");
        assert_eq!(
            metadata,
            ParamMetadata {
                default: ParamVal::Uint32(1500),
                min: ParamVal::Uint32(0),
                max: ParamVal::Uint32(u32::MAX),
                units: None,
                description: "Power".into(),
                reboot_required: true,
            }
        );

        message.param_id = *b"SM_MAIN_ALT_LONG";
        message.param_type = RapidParamType::Real32;
        message.default_value = 450.0;
        message.flags = ParamMetadataFlag::empty();
        message.units = *b"m\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
        let (id, metadata) = ParamMetadata::decode(&message, ParamEncoding::Bytewise).unwrap();
        assert_eq!(id, "SM_MAIN_ALT_LONG");
        assert_eq!(metadata.default, ParamVal::Float32(450.0));
        assert_eq!(metadata.units.as_deref(), Some("m"));
        assert!(!metadata.reboot_required);
    }

    #[test]
    fn members_are_recognized() {
        assert_eq!(
            Composite::of("LORA_KEY_3"),
            Some((Composite::Key, "LORA_KEY", 3))
        );
        assert_eq!(
            Composite::of("GCS_BIND_1"),
            Some((Composite::HopSeed, "GCS_BIND", 1))
        );
        assert_eq!(Composite::of("GCS_BIND_2"), None);
        assert_eq!(Composite::of("CAL_MAG_X"), None);
        assert_eq!(Composite::of("SM_MAIN_ALT"), None);
    }
}
