use std::sync::atomic::{AtomicBool, Ordering};

use egui::Color32;

pub const COLOR_INDICATOR_GOOD: Color32 = Color32::from_rgb(84, 195, 84);
pub const COLOR_INDICATOR_WARNING: Color32 = Color32::ORANGE;

pub const COLOR_INDICATOR_LIMITS: Color32 = Color32::RED;

// Amber corner tick on mode buttons: "expert only", same caution family as COLOR_INDICATOR_WARNING
// but distinct enough not to read as an active alarm.
pub const COLOR_INDICATOR_ADVANCED: Color32 = Color32::from_rgb(255, 179, 0);

// Cyan mode names mark autonomous modes; blue stays reserved for "selected".
pub const COLOR_INDICATOR_AUTONOMY: Color32 = Color32::from_rgb(0, 172, 193);

/// How far [`readable`] pulls the palette toward black. High contrast pulls further, because the
/// yellow-family hues do not clear AA at 0.35.
const DARKEN_LIGHT: f32 = 0.35;
const DARKEN_HIGH_CONTRAST: f32 = 0.5;

/// Floor on [`dim`]'s factor, so de-emphasis cannot fade a value under AA.
const DIM_FLOOR_HIGH_CONTRAST: f32 = 0.85;

/// Global because it is read from sites that only hold a `&Visuals`. Anything reading it must
/// branch on `visuals.dark_mode` first, or the forced-dark instrument subtrees come out wrong.
static HIGH_CONTRAST: AtomicBool = AtomicBool::new(false);

pub fn set_high_contrast(on: bool) {
    HIGH_CONTRAST.store(on, Ordering::Relaxed);
}

pub fn high_contrast() -> bool {
    HIGH_CONTRAST.load(Ordering::Relaxed)
}

// The palette is tuned against a dark canvas and washes out on a light one. Apply a
// `gamma_multiply` fade after this, never before: lerping toward opaque black puts the alpha back.
pub fn readable(color: Color32, visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        color
    } else if high_contrast() {
        color.lerp_to_gamma(Color32::BLACK, DARKEN_HIGH_CONTRAST)
    } else {
        color.lerp_to_gamma(Color32::BLACK, DARKEN_LIGHT)
    }
}

/// Fades `color` to de-emphasise it, floored so high contrast cannot fade anything under AA.
/// Instrument internals, which never see that theme, use `gamma_multiply` directly.
pub fn dim(color: Color32, factor: f32) -> Color32 {
    if high_contrast() {
        color.gamma_multiply(factor.max(DIM_FLOOR_HIGH_CONTRAST))
    } else {
        color.gamma_multiply(factor)
    }
}

/// <https://www.w3.org/TR/WCAG22/#dfn-relative-luminance>
fn relative_luminance(color: Color32) -> f32 {
    let channel = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };

    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

/// <https://www.w3.org/TR/WCAG22/#dfn-contrast-ratio>
pub fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let (a, b) = (relative_luminance(a), relative_luminance(b));
    let (lighter, darker) = if a > b { (a, b) } else { (b, a) };

    (lighter + 0.05) / (darker + 0.05)
}

pub fn text_on(fill: Color32) -> Color32 {
    if contrast_ratio(Color32::BLACK, fill) >= contrast_ratio(Color32::WHITE, fill) {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

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

/// The border is the button's only boundary while unselected, so high contrast drops the fade that
/// would take most of the palette under 3:1.
pub fn mode_border(custom_mode: u32, visuals: &egui::Visuals) -> Color32 {
    let color = readable(mode_color(custom_mode), visuals);

    if high_contrast() {
        color
    } else {
        color.gamma_multiply(0.7)
    }
}

// Instruments keep the cockpit palette whatever the window theme is. Call this inside the canvas,
// and inside any `Area` drawn over it, since an area builds its `Ui` from the context and misses
// the surrounding style.
pub fn instrument_visuals(ui: &mut egui::Ui) {
    ui.style_mut().visuals = egui::Visuals::dark();
}

/// Unlike the horizon and the dials, the schematic follows the window theme instead of pinning
/// itself dark with [`instrument_visuals`].
pub fn schematic_frame(style: &egui::Style) -> egui::Frame {
    if style.visuals.dark_mode {
        egui::Frame::dark_canvas(style)
    } else {
        // Not plain `Frame::canvas`: it fills with `extreme_bg_color`, which is also what the
        // readout boxes use, so they would sink into it.
        egui::Frame::canvas(style).fill(style.visuals.panel_fill)
    }
}

pub fn schematic_ink(visuals: &egui::Visuals) -> Color32 {
    visuals.strong_text_color()
}

/// Not `weak_text_color`: that is under 3:1 against either canvas, since `dark_canvas` is darker
/// than the panel the weak colour was mixed against.
pub fn schematic_line(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        visuals.text_color().gamma_multiply(0.65)
    } else {
        visuals.text_color()
    }
}

/// A closed valve, an unfilled vessel. A step away from the canvas, so the glyph reads as a hollow
/// outline rather than a hole.
pub fn schematic_void(visuals: &egui::Visuals) -> Color32 {
    visuals.extreme_bg_color
}

/// `window_stroke`'s width, but the schematic's line colour: `window_stroke`'s own is under 2:1
/// against either canvas.
pub fn schematic_box_stroke(visuals: &egui::Visuals) -> egui::Stroke {
    egui::Stroke::new(visuals.window_stroke().width, schematic_line(visuals))
}

/// Lightens a dark canvas and darkens a light one, so `alpha` means the same thing either way.
pub fn schematic_wash(visuals: &egui::Visuals, alpha: u8) -> Color32 {
    if visuals.dark_mode {
        Color32::from_white_alpha(alpha)
    } else {
        Color32::from_black_alpha(alpha)
    }
}

// `time` is egui's monotonic clock in seconds. Shared so every blinking box stays in phase.
pub fn blink_on(time: f64) -> bool {
    (time * 1.2).fract() < 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SC 1.4.3, for text that is not "large" (nothing in this UI is).
    const AA_TEXT: f32 = 4.5;
    /// SC 1.4.11, for control boundaries and meaningful graphics.
    const AA_NON_TEXT: f32 = 3.0;

    const INDICATORS: &[(&str, Color32)] = &[
        ("GOOD", COLOR_INDICATOR_GOOD),
        ("WARNING", COLOR_INDICATOR_WARNING),
        ("LIMITS", COLOR_INDICATOR_LIMITS),
        ("ADVANCED", COLOR_INDICATOR_ADVANCED),
        ("AUTONOMY", COLOR_INDICATOR_AUTONOMY),
    ];

    /// [`high_contrast`] is global state and `cargo test` runs each `#[test]` on its own thread.
    fn with_high_contrast<T>(on: bool, f: impl FnOnce() -> T) -> T {
        use std::sync::{Mutex, MutexGuard, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

        let guard: MutexGuard<'_, ()> = LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        set_high_contrast(on);
        let out = f();
        set_high_contrast(false);
        drop(guard);

        out
    }

    fn assert_at_least(ratio: f32, floor: f32, what: &str) {
        assert!(ratio >= floor, "{what}: {ratio:.2}:1, needs {floor:.1}:1");
    }

    #[test]
    fn the_high_contrast_theme_keeps_every_indicator_above_aa() {
        let visuals = crate::theme::high_contrast_visuals();

        with_high_contrast(true, || {
            for (name, color) in INDICATORS {
                let color = readable(*color, &visuals);

                for (surface, fill) in [
                    ("panel", visuals.panel_fill),
                    ("inset surface", visuals.extreme_bg_color),
                    ("striped row", visuals.faint_bg_color),
                ] {
                    assert_at_least(
                        contrast_ratio(color, fill),
                        AA_TEXT,
                        &format!("{name} on the {surface}"),
                    );
                }
            }
        });
    }

    #[test]
    fn the_high_contrast_theme_labels_indicator_fills_readably() {
        let visuals = crate::theme::high_contrast_visuals();

        with_high_contrast(true, || {
            for (name, color) in INDICATORS {
                let fill = readable(*color, &visuals);
                assert_at_least(
                    contrast_ratio(text_on(fill), fill),
                    AA_TEXT,
                    &format!("the automatic label colour on a {name} fill"),
                );
                assert_at_least(
                    contrast_ratio(fill, visuals.panel_fill),
                    AA_NON_TEXT,
                    &format!("a {name} fill against the panel"),
                );
            }
        });
    }

    #[test]
    fn the_high_contrast_theme_keeps_every_mode_border_identifiable() {
        let visuals = crate::theme::high_contrast_visuals();

        with_high_contrast(true, || {
            for mode in 0..10 {
                assert_at_least(
                    contrast_ratio(mode_border(mode, &visuals), visuals.extreme_bg_color),
                    AA_NON_TEXT,
                    &format!("the mode {mode} button border"),
                );
            }
        });
    }

    #[test]
    fn the_high_contrast_theme_keeps_body_and_weak_text_above_aa() {
        let visuals = crate::theme::high_contrast_visuals();

        assert_at_least(
            contrast_ratio(visuals.text_color(), visuals.panel_fill),
            AA_TEXT,
            "body text on the panel",
        );

        // `weak_text_color` fades by alpha, so it has to be composited before it can be measured.
        let weak = visuals.panel_fill.blend(visuals.weak_text_color());
        assert_at_least(
            contrast_ratio(weak, visuals.panel_fill),
            AA_TEXT,
            "weak text",
        );

        with_high_contrast(true, || {
            let dimmed = visuals
                .panel_fill
                .blend(dim(visuals.weak_text_color(), 0.5));
            assert_at_least(
                contrast_ratio(dimmed, visuals.panel_fill),
                AA_TEXT,
                "dimmed weak text (the \"no data\" placeholders)",
            );
        });
    }

    #[test]
    fn the_high_contrast_theme_gives_every_control_a_visible_border() {
        let visuals = crate::theme::high_contrast_visuals();
        let widgets = &visuals.widgets;

        for (name, widget) in [
            ("an idle control", &widgets.inactive),
            ("a hovered control", &widgets.hovered),
            ("a focused control", &widgets.active),
            ("an open control", &widgets.open),
            ("a separator", &widgets.noninteractive),
        ] {
            assert_at_least(
                contrast_ratio(widget.bg_stroke.color, visuals.panel_fill),
                AA_NON_TEXT,
                &format!("the border of {name} against the panel"),
            );
            assert_at_least(
                contrast_ratio(widget.text_color(), widget.weak_bg_fill),
                AA_TEXT,
                &format!("the label of {name}"),
            );
        }

        let focus = widgets.active.bg_stroke;
        assert!(
            focus.width >= 2.0,
            "the focus ring is {}px, SC 2.4.11 asks for 2px",
            focus.width
        );
        assert_at_least(
            contrast_ratio(focus.color, widgets.active.weak_bg_fill),
            AA_NON_TEXT,
            "the focus ring against the control it surrounds",
        );
    }

    /// The same fill backs a selected button and a `TextEdit` highlight, which draw different text
    /// over it.
    #[test]
    fn the_high_contrast_theme_keeps_selections_readable() {
        let visuals = crate::theme::high_contrast_visuals();
        let fill = visuals.selection.bg_fill;

        assert_at_least(
            contrast_ratio(visuals.selection.stroke.color, fill),
            AA_TEXT,
            "the label of a selected button",
        );
        assert_at_least(
            contrast_ratio(visuals.text_color(), fill),
            AA_TEXT,
            "body text behind a selection highlight",
        );
    }

    /// The alert line inverts rather than fades, so both halves have to clear AA.
    #[test]
    fn both_halves_of_the_high_contrast_alert_blink_clear_aa() {
        let visuals = crate::theme::high_contrast_visuals();

        with_high_contrast(true, || {
            for (name, color) in [
                ("critical", COLOR_INDICATOR_LIMITS),
                ("caution", COLOR_INDICATOR_WARNING),
            ] {
                let color = readable(color, &visuals);

                assert_at_least(
                    contrast_ratio(color, visuals.extreme_bg_color),
                    AA_TEXT,
                    &format!("the unlit half of the {name} alert blink"),
                );
                assert_at_least(
                    contrast_ratio(text_on(color), color),
                    AA_TEXT,
                    &format!("the lit half of the {name} alert blink"),
                );
            }
        });
    }

    /// `Frame::dark_canvas` fills with a near-opaque black, so the canvas has to be composited
    /// onto the panel before it can be measured.
    fn schematic_canvases() -> Vec<(&'static str, egui::Visuals, Color32, bool)> {
        [
            ("dark", egui::Visuals::dark(), false),
            ("light", egui::Visuals::light(), false),
            ("high contrast", crate::theme::high_contrast_visuals(), true),
        ]
        .into_iter()
        .map(|(name, visuals, hc)| {
            let style = egui::Style {
                visuals: visuals.clone(),
                ..Default::default()
            };
            let canvas = visuals.panel_fill.blend(schematic_frame(&style).fill);
            (name, visuals, canvas, hc)
        })
        .collect()
    }

    #[test]
    fn the_schematic_reads_on_every_canvas() {
        for (theme, visuals, canvas, hc) in schematic_canvases() {
            with_high_contrast(hc, || {
                assert_at_least(
                    contrast_ratio(schematic_ink(&visuals), canvas),
                    AA_TEXT,
                    &format!("the schematic's ink on the {theme} canvas"),
                );
                assert_at_least(
                    contrast_ratio(schematic_line(&visuals), canvas),
                    AA_NON_TEXT,
                    &format!("the schematic's line work on the {theme} canvas"),
                );
                assert_at_least(
                    contrast_ratio(schematic_box_stroke(&visuals).color, canvas),
                    AA_NON_TEXT,
                    &format!("a readout box outline on the {theme} canvas"),
                );

                // A closed valve's outline has to separate it from the canvas *and* its own fill.
                let void = schematic_void(&visuals);
                assert_at_least(
                    contrast_ratio(schematic_ink(&visuals), void),
                    AA_NON_TEXT,
                    &format!("a closed valve's outline against its fill on the {theme} canvas"),
                );
            });
        }
    }

    #[test]
    fn every_fluid_colour_reads_on_every_canvas() {
        use crate::panes::propulsion_fluid_colors;

        for (theme, visuals, canvas, hc) in schematic_canvases() {
            with_high_contrast(hc, || {
                for (fluid, color) in propulsion_fluid_colors() {
                    let color = readable(color, &visuals);
                    assert_at_least(
                        contrast_ratio(color, canvas),
                        AA_NON_TEXT,
                        &format!("{fluid} on the {theme} canvas"),
                    );
                    assert_at_least(
                        contrast_ratio(color, schematic_void(&visuals)),
                        AA_NON_TEXT,
                        &format!("{fluid} inside a vessel on the {theme} canvas"),
                    );
                }
            });
        }
    }

    #[test]
    fn the_dark_theme_is_left_alone() {
        let dark = egui::Visuals::dark();

        with_high_contrast(true, || {
            for (_, color) in INDICATORS {
                assert_eq!(
                    readable(*color, &dark),
                    *color,
                    "the high-contrast theme must not reach into the forced-dark instruments",
                );
            }
        });
    }
}
