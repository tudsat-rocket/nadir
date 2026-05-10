use std::time::Duration;

use maviola::protocol::ComponentId;
use mavspec::rust::dialects::Common;
use mavspec::rust::dialects::ardupilotmega::enums::{CopterMode, PlaneMode};
use mavspec::rust::dialects::common::{
    enums::{MavAutopilot, MavCmd, MavModeProperty, MavStandardMode, MavType},
    messages::{AvailableModes, CommandLong},
};

use crate::System;
use crate::protocols::{Gatherable, gather};

impl Gatherable for AvailableModes {
    type InitialRequest = CommandLong;
    type SpecificRequest = CommandLong;

    fn index(&self) -> usize {
        // AVAILABLE_MODES uses 1-based indices
        (self.mode_index as usize) - 1
    }

    fn count(&self) -> usize {
        self.number_modes as usize
    }

    fn unpack(msg: Common) -> Option<Self> {
        match msg {
            Common::AvailableModes(inner) => Some(inner),
            _ => None,
        }
    }

    fn initial_request(system_id: u8, component_id: u8) -> Self::InitialRequest {
        CommandLong {
            target_system: system_id,
            target_component: component_id,
            command: MavCmd::RequestMessage,
            param1: AvailableModes::ID as f32,
            param2: 0.0,
            ..Default::default()
        }
    }

    fn specific_request(system_id: u8, component_id: u8, index: usize) -> Self::SpecificRequest {
        CommandLong {
            target_system: system_id,
            target_component: component_id,
            command: MavCmd::RequestMessage,
            param1: AvailableModes::ID as f32,
            param2: (index + 1) as f32,
            ..Default::default()
        }
    }
}

// ArduPilot hardcodes a zero MAV_MODE_PROPERTY bitmask for every AVAILABLE_MODES entry, and
// ArduCopter doesn't even respond to AVAILABLE_MODES, so we make some up here.

fn arducopter_mode_properties(cm: CopterMode) -> MavModeProperty {
    use MavModeProperty as P;
    match cm {
        CopterMode::Stabilize
        | CopterMode::AltHold
        | CopterMode::Poshold
        | CopterMode::Flowhold
        | CopterMode::Loiter
        | CopterMode::Land
        | CopterMode::Brake => P::empty(),
        CopterMode::Acro | CopterMode::Autotune | CopterMode::Throw | CopterMode::Turtle => {
            P::ADVANCED
        }
        CopterMode::Auto
        | CopterMode::Guided
        | CopterMode::Rtl
        | CopterMode::Circle
        | CopterMode::SmartRtl
        | CopterMode::Follow => P::AUTO_MODE,
        CopterMode::Drift | CopterMode::Sport | CopterMode::Flip | CopterMode::Systemid => {
            P::NOT_USER_SELECTABLE | P::ADVANCED
        }
        CopterMode::AvoidAdsb => P::NOT_USER_SELECTABLE | P::AUTO_MODE,
        CopterMode::Autorotate
        | CopterMode::AutoRtl
        | CopterMode::GuidedNogps
        | CopterMode::Zigzag => P::NOT_USER_SELECTABLE | P::ADVANCED | P::AUTO_MODE,
    }
}

fn arduplane_mode_properties(pm: PlaneMode) -> MavModeProperty {
    use MavModeProperty as P;
    match pm {
        PlaneMode::Manual
        | PlaneMode::Stabilize
        | PlaneMode::Training
        | PlaneMode::FlyByWireA
        | PlaneMode::FlyByWireB
        | PlaneMode::Cruise
        | PlaneMode::Qstabilize
        | PlaneMode::Qhover => P::empty(),

        PlaneMode::Acro | PlaneMode::Autotune | PlaneMode::Qacro => P::ADVANCED,

        PlaneMode::Auto
        | PlaneMode::Rtl
        | PlaneMode::Loiter
        | PlaneMode::Takeoff
        | PlaneMode::Guided
        | PlaneMode::Qloiter
        | PlaneMode::Qland
        | PlaneMode::Qrtl
        | PlaneMode::Autoland => P::AUTO_MODE,

        PlaneMode::Qautotune | PlaneMode::Thermal => P::ADVANCED | P::AUTO_MODE,

        PlaneMode::Initializing => P::NOT_USER_SELECTABLE,
        PlaneMode::Circle | PlaneMode::AvoidAdsb | PlaneMode::LoiterAltQland => {
            P::NOT_USER_SELECTABLE | P::AUTO_MODE
        }
    }
}

// Replace the (zero) firmware-supplied properties with our own classification, then bucket-sort.
fn fixup_arduplane_modes(modes: &mut [AvailableModes]) {
    for m in modes.iter_mut() {
        let Ok(pm) = PlaneMode::try_from(m.custom_mode as u8) else {
            continue;
        };
        m.properties = arduplane_mode_properties(pm);
    }
    sort_and_reindex(modes);
}

fn property_sort_key(p: MavModeProperty) -> u8 {
    if p.contains(MavModeProperty::NOT_USER_SELECTABLE) {
        3
    } else if p.contains(MavModeProperty::ADVANCED) {
        2
    } else {
        u8::from(p.contains(MavModeProperty::AUTO_MODE))
    }
}

fn sort_and_reindex(modes: &mut [AvailableModes]) {
    modes.sort_by_key(|m| property_sort_key(m.properties));
    let count = modes.len() as u8;
    for (i, m) in modes.iter_mut().enumerate() {
        m.number_modes = count;
        m.mode_index = i as u8;
    }
}

fn arducopter_modes() -> Vec<AvailableModes> {
    let mut modes: Vec<AvailableModes> = CopterMode::entries()
        .map(|cm| {
            let mode_name_str = format!("{cm:?}");
            let mode_name_bytes = mode_name_str.as_bytes();

            let mut mode_name = [0x00; 35];
            mode_name[..(mode_name_bytes.len())].copy_from_slice(mode_name_bytes);

            AvailableModes {
                number_modes: 0,
                mode_index: 0,
                standard_mode: MavStandardMode::NonStandard,
                custom_mode: u32::from(cm.value()),
                properties: arducopter_mode_properties(cm),
                mode_name,
            }
        })
        .collect();

    sort_and_reindex(&mut modes);
    modes
}

pub async fn discover_available_modes(
    system: System,
    component_id: ComponentId,
    mut message_rx: tokio::sync::broadcast::Receiver<Common>,
) {
    // Wait for a heartbeat so we know our firmware, because...
    let heartbeat = loop {
        if let Ok(Common::Heartbeat(hb)) = message_rx.recv().await {
            break hb;
        }
    };

    // Arducopter does not support this protocol for some reason.
    if heartbeat.autopilot == MavAutopilot::Ardupilotmega && heartbeat.type_ != MavType::FixedWing {
        let modes = arducopter_modes();
        *system.available_modes.lock().unwrap() = Some(modes);
        return;
    }

    let is_arduplane =
        heartbeat.autopilot == MavAutopilot::Ardupilotmega && heartbeat.type_ == MavType::FixedWing;

    // Gather AVAILABLE_MODES data.
    tracing::debug!(system_id = system.system_id, "Starting mode discovery.");

    let mut first = true;
    loop {
        match gather(&system, component_id, &mut message_rx, None).await {
            Ok(mut modes) => {
                if is_arduplane {
                    fixup_arduplane_modes(&mut modes);
                }
                *system.available_modes.lock().unwrap() = Some(modes);
                break;
            }
            Err(_res) => {
                if first {
                    tracing::error!("Mode discovery failed, will keep retrying.");
                }

                tokio::time::sleep(Duration::from_millis(10000)).await;
            }
        }

        first = false;
    }
}
