use std::collections::HashMap;
use std::time::Duration;

use chrono::TimeDelta;
use nadir_core::System;

use eframe::egui;
use egui::{Color32, RichText};
use mavspec::rust::dialects::common::enums::{
    MavCmd, MavResult, MavSysStatusSensor, MavSysStatusSensorExtended,
};
use mavspec::rust::dialects::common::messages::{
    CommandAck, LinkNodeStatus, RadioStatus, SysStatus,
};
use rapid_dialect::rapid;

use crate::colors::{
    COLOR_INDICATOR_LIMITS, COLOR_INDICATOR_WARNING, blink_on, dim, high_contrast, readable,
    text_on,
};
use crate::widgets::{TEXT_SIZE, small_text};

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

/// Two missed heartbeats at the 1 Hz zenith sends its per-node ones at.
pub(crate) const STALE_AFTER: TimeDelta = TimeDelta::seconds(3);

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

    // A stale report outlives the link that carried it, see [`link_silence`].
    let uplink_silent = system
        .uplink_silence()
        .is_some_and(|d| d >= UPLINK_SILENT_AFTER);
    if let Ok(lns) = system.last_message::<LinkNodeStatus>()
        && !uplink_silent
    {
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

/// Gaps shorter than these are ordinary packet spacing, not a silent link. The downlink one matches
/// the 1 s window the packet rates are taken over; the uplink one has to outlast the interval
/// `LINK_NODE_STATUS` arrives at (1 s from zenith, 2 s as requested), since that is the only news
/// of the uplink there is.
const DOWNLINK_SILENT_AFTER: Duration = Duration::from_secs(1);
const UPLINK_SILENT_AFTER: Duration = Duration::from_secs(3);

/// How long the downlink and uplink have each been silent, once that is longer than ordinary packet
/// spacing; `None` while the link is fine or there is no live link to time.
pub(crate) fn link_silence(ui: &egui::Ui, system: &System) -> (Option<Duration>, Option<Duration>) {
    let downlink = system.downlink_silence();
    let uplink = system.uplink_silence();

    // Nothing else repaints a link that went quiet, so keep the timers ticking (and appearing) on
    // their own.
    if downlink.is_some() || uplink.is_some() {
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }

    (
        downlink.filter(|d| *d >= DOWNLINK_SILENT_AFTER),
        uplink.filter(|d| *d >= UPLINK_SILENT_AFTER),
    )
}

/// A link silence as a short counting-up timer, e.g. "4.2s" or "3m07s".
pub(crate) fn format_silence(silence: Duration) -> String {
    let secs = silence.as_secs_f32();
    if secs < 60.0 {
        format!("{secs:.1}s")
    } else {
        let secs = silence.as_secs();
        format!("{}m{:02}s", secs / 60, secs % 60)
    }
}

/// [`format_silence`], in red.
pub(crate) fn silence_text(ui: &egui::Ui, silence: Duration) -> RichText {
    RichText::new(format_silence(silence))
        .monospace()
        .color(readable(COLOR_INDICATOR_LIMITS, ui.visuals()))
}

/// Which severity an [`AlertLine`] renders. The two tiers stack as separate rows in the status
/// strip: red critical over amber caution.
pub enum AlertTier {
    /// Red: command rejections, RF uplink/downlink quality alarms, lost components and changed
    /// `SYS_STATUS` error counters - the "you may be losing the vehicle" set.
    Critical,
    /// Amber: failed onboard sensors and (once the propulsion protocol exposes them) valve error
    /// states.
    Caution,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Counter {
    One,
    Two,
    Three,
    Comm,
}

impl Counter {
    const ALL: [Self; 4] = [Self::One, Self::Two, Self::Three, Self::Comm];

    fn value(self, s: &SysStatus) -> u16 {
        match self {
            Self::One => s.errors_count1,
            Self::Two => s.errors_count2,
            Self::Three => s.errors_count3,
            Self::Comm => s.errors_comm,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::One => "ERROR 1",
            Self::Two => "ERROR 2",
            Self::Three => "ERROR 3",
            Self::Comm => "COMM ERROR",
        }
    }
}

/// The two kinds of sticky, per-component alarm this row tracks: a `SYS_STATUS` error counter
/// moving, or the component dropping off entirely.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Alarm {
    Counter(Counter),
    Lost,
}

/// Tracked state for one `(component, alarm)`. `last_value` is whatever we last polled - the
/// counter reading, or the component's last-heard time in epoch millis for `Alarm::Lost` - purely
/// to detect the *next* change; `pending` is the sticky, operator-visible alarm.
///
/// `pending` is intentionally not recomputed from `last_value` each frame: a counter alerts the
/// moment it moves away from the value we last polled, and then stays alerted regardless of where
/// it goes next. Some firmware packs these into a handful of bits and wraps rather than saturates,
/// so a live counter can cycle back through an old reading while still erroring; comparing "is it
/// different from where it was" on every frame would let the alert flicker off right then, with no
/// operator click. Tracking "did it change, ever, since the last ack" instead is wrap-proof no
/// matter how narrow the counter is or how it bounces around.
#[derive(Clone, Copy, Default)]
struct AlarmState {
    last_value: i64,
    pending: bool,
}

type AlarmStates = HashMap<(u8, Alarm), AlarmState>;

struct Token {
    text: String,
    /// Clicking the token acknowledges this alarm, until it changes again.
    ack: Option<(u8, Alarm)>,
}

impl From<String> for Token {
    fn from(text: String) -> Self {
        Self { text, ack: None }
    }
}

/// One severity row of the status strip: command NACKs, RF alarms, lost components and changed
/// error counters in red (critical), failed sensors in amber (caution). Text only; a row stays dark
/// unless something in its tier is wrong.
pub struct AlertLine<'a> {
    pub system: &'a System,
    pub tier: AlertTier,
}

impl AlertLine<'_> {
    /// Polls one alarm's current reading against its tracked state, latching `pending` on any
    /// change and always advancing `last_value` so the *next* change (even back to an old value)
    /// is caught too. Pushes a token for `name` while the alarm is pending.
    fn poll(
        states: &mut AlarmStates,
        out: &mut Vec<Token>,
        id: u8,
        alarm: Alarm,
        value: i64,
        // Whether the very first sighting of this alarm should itself alert. A pre-existing
        // non-zero error count when we first connect isn't new news, but a component that's
        // already lost the moment we notice it most certainly is.
        alert_on_first_sighting: bool,
        text: impl FnOnce() -> String,
    ) {
        let key = (id, alarm);
        let entry = states.entry(key).or_insert(AlarmState {
            last_value: value,
            pending: alert_on_first_sighting,
        });
        if entry.last_value != value {
            entry.last_value = value;
            entry.pending = true;
        }
        if entry.pending {
            out.push(Token {
                text: text(),
                ack: Some(key),
            });
        }
    }

    fn tokens(&self, states: &mut AlarmStates) -> Vec<Token> {
        match self.tier {
            AlertTier::Critical => {
                let mut out: Vec<Token> = Vec::new();
                if let Some(nack) = last_nack(self.system) {
                    out.push(nack.into());
                }
                let (down, up) = link_quality(self.system);
                if let Some(q) = down.filter(|q| *q < LINK_ALARM_QUALITY) {
                    out.push(format!("DOWNLINK {:.0}%", q * 100.0).into());
                }
                if let Some(q) = up.filter(|q| *q < LINK_ALARM_QUALITY) {
                    out.push(format!("UPLINK {:.0}%", q * 100.0).into());
                }

                let now = self.system.now();
                for (id, last) in self.system.components() {
                    let name = if id == 0x01 {
                        "FC".to_owned()
                    } else {
                        format!("COMP 0x{id:02x}")
                    };

                    if now - last > STALE_AFTER {
                        Self::poll(
                            states,
                            &mut out,
                            id,
                            Alarm::Lost,
                            last.timestamp_millis(),
                            true,
                            || format!("{name} LOST"),
                        );
                    }

                    let Ok(latest) = self.system.last_message_from::<SysStatus>(id) else {
                        continue;
                    };
                    for counter in Counter::ALL {
                        let value = counter.value(&latest);
                        Self::poll(
                            states,
                            &mut out,
                            id,
                            Alarm::Counter(counter),
                            i64::from(value),
                            false,
                            || format!("{name} {}", counter.label()),
                        );
                    }
                }
                out
            }
            // TODO: valve error states once the propulsion protocol exposes them.
            AlertTier::Caution => self
                .system
                .last_message::<SysStatus>()
                .ok()
                .map(|s| sensor_lists(&s).0.into_iter().map(Token::from).collect())
                .unwrap_or_default(),
        }
    }
}

/// Acks for rapid-only commands fail the common decode and are stored as their own type.
fn last_nack(system: &System) -> Option<String> {
    let common = system.last_message_at::<CommandAck>().ok().map(|(t, ack)| {
        let failed = !matches!(ack.result, MavResult::Accepted | MavResult::InProgress)
            && !matches!(
                ack.command,
                MavCmd::RequestMessage | MavCmd::SetMessageInterval
            );
        (t, failed.then(|| format!("NACK {:?}", ack.command)))
    });
    let rapid = system
        .last_message_at::<rapid::messages::CommandAck>()
        .ok()
        .map(|(t, ack)| {
            let failed = !matches!(
                ack.result,
                rapid::enums::MavResult::Accepted | rapid::enums::MavResult::InProgress
            );
            (t, failed.then(|| format!("NACK {:?}", ack.command)))
        });

    [common, rapid]
        .into_iter()
        .flatten()
        .max_by_key(|(t, _)| *t)?
        .1
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
        let states_id = egui::Id::new(("alert_acks", self.system.system_id));
        let mut states: AlarmStates = ui.data(|d| d.get_temp(states_id).unwrap_or_default());
        let tokens = self.tokens(&mut states);

        let lit = blink_on(ui.input(|i| i.time));

        // Fading the text out leaves a critical alarm barely legible for half of every cycle, so
        // high contrast inverts the token instead. The frame is drawn either way, so the geometry
        // does not move with the blink.
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
                        .show(ui, |ui| {
                            let Some(key) = token.ack else {
                                small_text(ui, &token.text, ink);
                                return;
                            };
                            let clicked = ui
                                .add(
                                    egui::Label::new(
                                        RichText::new(&token.text)
                                            .monospace()
                                            .size(TEXT_SIZE)
                                            .color(ink),
                                    )
                                    .sense(egui::Sense::click())
                                    .wrap_mode(egui::TextWrapMode::Truncate),
                                )
                                .on_hover_text("Click to acknowledge")
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked();
                            if clicked {
                                states.entry(key).or_default().pending = false;
                            }
                        });
                }
            })
            .response;

        ui.data_mut(|d| d.insert_temp(states_id, states));

        // The event-driven repaint only fires on traffic; keep the blink animating even on a quiet
        // link.
        if !tokens.is_empty() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }

        response
    }
}
