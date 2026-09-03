use nadir_core::System;

use eframe::egui;
use egui::Color32;
use mavspec::rust::dialects::common::enums::{
    MavCmd, MavResult, MavSysStatusSensor, MavSysStatusSensorExtended,
};
use mavspec::rust::dialects::common::messages::{
    CommandAck, LinkNodeStatus, RadioStatus, SysStatus,
};

use crate::colors::{
    COLOR_INDICATOR_LIMITS, COLOR_INDICATOR_WARNING, blink_on, dim, high_contrast, readable,
    text_on,
};
use crate::widgets::small_text;

fn short_sensor_name(name: &str) -> String {
    let name = name
        .replace("MAV_SYS_STATUS_", "")
        .replace("SENSOR_", "")
        .replace('_', " ");
    // "PREARM CHECK" alone reads like a checklist item, not a caution.
    if name == "PREARM CHECK" {
        "PREARM CHECK FAILED".into()
    } else {
        name
    }
}

/// Splits the `SYS_STATUS` sensor bitmasks into (failed, disabled) short names. A disabled recovery
/// system means "disarmed" and is reported as a failure, not as a benign disabled subsystem.
fn sensor_lists(s: &SysStatus) -> (Vec<String>, Vec<String>) {
    let mut failed = Vec::new();
    let mut disabled = Vec::new();

    for (name, stat) in MavSysStatusSensor::all().iter_names() {
        if stat == MavSysStatusSensor::MAV_SYS_STATUS_EXTENSION_USED
            || !s.onboard_control_sensors_present.contains(stat)
        {
            continue;
        }
        if !s.onboard_control_sensors_enabled.contains(stat) {
            disabled.push(short_sensor_name(name));
        } else if !s.onboard_control_sensors_health.contains(stat) {
            failed.push(short_sensor_name(name));
        }
    }

    for (name, stat) in MavSysStatusSensorExtended::all().iter_names() {
        if !s.onboard_control_sensors_present_extended.contains(stat) {
            continue;
        }
        let enabled = s.onboard_control_sensors_enabled_extended.contains(stat);
        let healthy = s.onboard_control_sensors_health_extended.contains(stat);
        if stat == MavSysStatusSensorExtended::MAV_SYS_STATUS_RECOVERY_SYSTEM {
            if !enabled || !healthy {
                failed.push("RECOVERY DISARMED".into());
            }
        } else if !enabled {
            disabled.push(short_sensor_name(name));
        } else if !healthy {
            failed.push(short_sensor_name(name));
        }
    }

    (failed, disabled)
}

/// Below this quality (1.0 = perfect) an RF link is a flight-critical alarm, not a passing dip:
/// neither rockets nor multirotors fly open-loop, so a degraded uplink or downlink belongs on the
/// red line.
const LINK_ALARM_QUALITY: f32 = 0.5;

/// Worst downlink and uplink quality (1.0 = perfect; `None` = no data) across our local receive
/// stats and any `RADIO_STATUS` / `LINK_NODE_STATUS` the vehicle reports. Mirrors the per-link math
/// in the Links pane, and is also what the vitals columns show per direction.
pub(crate) fn link_quality(system: &System) -> (Option<f32>, Option<f32>) {
    let mut downlink: Vec<f32> = Vec::new();
    let mut uplink: Vec<f32> = Vec::new();

    let mut channels = system.channels();
    let down_packets: f32 = channels
        .iter_mut()
        .map(|(_, s)| s.received_packet_rate())
        .sum();
    if down_packets > 0.0 {
        let loss: f32 = channels
            .iter_mut()
            .map(|(_, s)| s.packet_loss() * s.received_packet_rate())
            .sum();
        downlink.push(1.0 - loss / down_packets);
    }

    if let Ok(lns) = system.last_message::<LinkNodeStatus>() {
        let total = lns.messages_received as f32 + lns.messages_lost as f32;
        if total > 0.0 {
            uplink.push(lns.messages_received as f32 / total);
        }
    }
    if let Ok(rs) = system.last_message::<RadioStatus>() {
        downlink.push(1.0 - f32::from(rs.rxerrors) / 100.0);
        uplink.push(1.0 - f32::from(rs.fixed) / 100.0);
    }

    let worst = |v: Vec<f32>| {
        v.into_iter()
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    };
    (worst(downlink), worst(uplink))
}

/// Which severity an [`AlertLine`] renders. The two tiers stack as separate rows in the status
/// strip: red critical over amber caution.
pub enum AlertTier {
    /// Red: command rejections and RF uplink/downlink quality alarms - the "you may be losing the
    /// vehicle" set.
    Critical,
    /// Amber: failed onboard sensors and (once the propulsion protocol exposes them) valve error
    /// states.
    Caution,
}

/// One severity row of the status strip: command NACKs and RF alarms in red (critical), failed
/// sensors in amber (caution). Text only; a row stays dark unless something in its tier is wrong.
pub struct AlertLine<'a> {
    pub system: &'a System,
    pub tier: AlertTier,
}

impl AlertLine<'_> {
    fn tokens(&self) -> Vec<String> {
        match self.tier {
            AlertTier::Critical => {
                let mut out = Vec::new();
                if let Ok(ack) = self.system.last_message::<CommandAck>()
                    && !matches!(ack.result, MavResult::Accepted | MavResult::InProgress)
                    && !matches!(
                        ack.command,
                        MavCmd::RequestMessage | MavCmd::SetMessageInterval
                    )
                {
                    out.push(format!("NACK {:?}", ack.command));
                }
                let (down, up) = link_quality(self.system);
                if let Some(q) = down.filter(|q| *q < LINK_ALARM_QUALITY) {
                    out.push(format!("DOWNLINK {:.0}%", q * 100.0));
                }
                if let Some(q) = up.filter(|q| *q < LINK_ALARM_QUALITY) {
                    out.push(format!("UPLINK {:.0}%", q * 100.0));
                }
                out
            }
            // TODO: valve error states once the propulsion protocol exposes them.
            AlertTier::Caution => self
                .system
                .last_message::<SysStatus>()
                .ok()
                .map(|s| sensor_lists(&s).0)
                .unwrap_or_default(),
        }
    }
}

impl egui::Widget for AlertLine<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let color = readable(
            match self.tier {
                AlertTier::Critical => COLOR_INDICATOR_LIMITS,
                AlertTier::Caution => COLOR_INDICATOR_WARNING,
            },
            ui.visuals(),
        );
        let tokens = self.tokens();

        let lit = blink_on(ui.input(|i| i.time));

        // The default themes blink by fading the text out, which takes a critical alarm down to
        // 1.4:1 for half of every cycle. The high-contrast theme inverts the token instead: the
        // geometry never moves and both halves of the blink stay well over AA. See
        // `docs/accessibility-review.md` §3.4.
        let (fill, ink) = if high_contrast() {
            if lit {
                (color, text_on(color))
            } else {
                (Color32::TRANSPARENT, color)
            }
        } else if lit {
            (Color32::TRANSPARENT, color)
        } else {
            (Color32::TRANSPARENT, dim(color, 0.35))
        };

        let response = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                for token in &tokens {
                    egui::Frame::new()
                        .fill(fill)
                        .inner_margin(egui::Margin::symmetric(3, 0))
                        .show(ui, |ui| small_text(ui, token, ink));
                }
            })
            .response;

        // The event-driven repaint only fires on traffic; keep the blink animating even on a quiet
        // link.
        if !tokens.is_empty() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }

        response
    }
}
