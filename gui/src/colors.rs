use egui::Color32;

pub const COLOR_INDICATOR_GOOD: Color32 = Color32::from_rgb(84, 195, 84);
pub const COLOR_INDICATOR_WARNING: Color32 = Color32::ORANGE;

pub const COLOR_INDICATOR_LIMITS: Color32 = Color32::RED;

pub const COLOR_INDICATOR_AUTONOMY: Color32 = Color32::from_rgb(123, 31, 162);

// True during the "on" half of the standard ~1.2 Hz warning-blink cycle. `time`
// is egui's monotonic clock in seconds. Shared so every blinking box (valves,
// overview indicators) stays in phase.
pub fn blink_on(time: f64) -> bool {
    (time * 1.2).fract() < 0.5
}
