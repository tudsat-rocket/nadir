use egui::Color32;

pub const COLOR_INDICATOR_GOOD: Color32 = Color32::from_rgb(84, 195, 84);
pub const COLOR_INDICATOR_WARNING: Color32 = Color32::ORANGE;

pub const COLOR_INDICATOR_LIMITS: Color32 = Color32::RED;

// Amber corner tick on mode buttons: "expert only", same caution family as COLOR_INDICATOR_WARNING
// but distinct enough not to read as an active alarm.
pub const COLOR_INDICATOR_ADVANCED: Color32 = Color32::from_rgb(255, 179, 0);

// Cyan mode names mark autonomous modes; blue stays reserved for "selected".
pub const COLOR_INDICATOR_AUTONOMY: Color32 = Color32::from_rgb(0, 172, 193);

pub fn mode_color(custom_mode: u32) -> Color32 {
    const PALETTE: &[Color32] = &[
        Color32::from_rgb(127, 127, 127),
        Color32::from_rgb(44, 160, 44),
        Color32::from_rgb(31, 119, 180),
        Color32::from_rgb(214, 39, 40),
        Color32::from_rgb(148, 103, 189),
        Color32::from_rgb(140, 86, 75),
        Color32::from_rgb(227, 119, 194),
        Color32::from_rgb(255, 127, 14),
        Color32::from_rgb(188, 189, 34),
        Color32::from_rgb(23, 190, 207),
    ];
    PALETTE[custom_mode as usize % PALETTE.len()]
}

// True during the "on" half of the standard ~1.2 Hz warning-blink cycle. `time`
// is egui's monotonic clock in seconds. Shared so every blinking box (valves,
// overview indicators) stays in phase.
pub fn blink_on(time: f64) -> bool {
    (time * 1.2).fract() < 0.5
}
