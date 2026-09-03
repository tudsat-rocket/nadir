//! Window themes.
//!
//! egui has no third theme slot, so [`Theme::HighContrast`] is applied as a replacement *light*
//! style: the preference is pinned to light and `Options::light_style` is swapped out.

use egui::style::{Selection, WidgetVisuals, Widgets};
use egui::{Color32, CornerRadius, Stroke, Style, Visuals};

use nadir_core::settings::Theme;

use crate::colors;

const SURFACE: Color32 = Color32::WHITE;
const SURFACE_INSET: Color32 = Color32::from_gray(237);
const SURFACE_STRIPE: Color32 = Color32::from_gray(232);

const BORDER: Color32 = Color32::from_gray(26);
const BORDER_WEAK: Color32 = Color32::from_gray(90);

const FILL_HOVERED: Color32 = Color32::from_gray(214);
const FILL_ACTIVE: Color32 = Color32::from_gray(184);

/// SC 2.4.11 wants a focus indicator at least 2 px thick, and egui derives the focused look from
/// `widgets.active`.
const FOCUS_WIDTH: f32 = 2.0;
const BORDER_WIDTH: f32 = 1.0;

/// Must equal [`BORDER_WIDTH`], and must be the same for every widget state, or a button changes
/// size when hovered - see `buttons_keep_their_size_in_every_state`.
const EXPANSION: f32 = BORDER_WIDTH;

/// Also the text-selection highlight in a `TextEdit`, where the text stays the body colour rather
/// than [`SELECTION_FG`].
const SELECTION_BG: Color32 = Color32::from_rgb(0xa8, 0xd4, 0xff);
const SELECTION_FG: Color32 = Color32::from_rgb(0x00, 0x2a, 0x45);

/// Higher than egui's 0.6, to leave headroom for the further [`colors::dim`] fade on top.
const WEAK_TEXT_ALPHA: f32 = 0.7;
const DISABLED_ALPHA: f32 = 0.6;

pub fn apply(ctx: &egui::Context, theme: Theme) {
    colors::set_high_contrast(theme == Theme::HighContrast);

    // Rebuilt from scratch rather than mutated in place, so switching *out* of high contrast
    // restores egui's light theme instead of leaving half the overrides behind.
    ctx.set_style_of(
        egui::Theme::Light,
        if theme == Theme::HighContrast {
            high_contrast_style()
        } else {
            egui::Theme::Light.default_style()
        },
    );

    ctx.set_theme(match theme {
        Theme::System => egui::ThemePreference::System,
        Theme::Dark => egui::ThemePreference::Dark,
        Theme::Light | Theme::HighContrast => egui::ThemePreference::Light,
    });
}

pub fn label(theme: Theme) -> &'static str {
    match theme {
        Theme::System => "System",
        Theme::Dark => "Dark",
        Theme::Light => "Light",
        Theme::HighContrast => "High Contrast",
    }
}

pub const ALL: [Theme; 4] = [
    Theme::System,
    Theme::Dark,
    Theme::Light,
    Theme::HighContrast,
];

fn high_contrast_style() -> Style {
    let mut style = egui::Theme::Light.default_style();
    style.visuals = high_contrast_visuals();
    style
}

pub(crate) fn high_contrast_visuals() -> Visuals {
    let control =
        |fill: Color32, border: Color32, border_width: f32, text_width: f32| WidgetVisuals {
            weak_bg_fill: fill,
            bg_fill: fill,
            bg_stroke: Stroke::new(border_width, border),
            fg_stroke: Stroke::new(text_width, Color32::BLACK),
            corner_radius: CornerRadius::same(2),
            expansion: EXPANSION,
        };

    Visuals {
        widgets: Widgets {
            noninteractive: control(SURFACE, BORDER_WEAK, BORDER_WIDTH, 1.0),
            inactive: control(SURFACE, BORDER, BORDER_WIDTH, 1.0),
            hovered: control(FILL_HOVERED, BORDER, FOCUS_WIDTH, 1.5),
            // Also the focused state: egui picks `active` for a widget with keyboard focus.
            active: control(FILL_ACTIVE, Color32::BLACK, FOCUS_WIDTH, 2.0),
            open: control(SURFACE, BORDER, BORDER_WIDTH, 1.0),
        },

        selection: Selection {
            bg_fill: SELECTION_BG,
            stroke: Stroke::new(1.0, SELECTION_FG),
        },

        panel_fill: SURFACE,
        window_fill: SURFACE,
        extreme_bg_color: SURFACE_INSET,
        faint_bg_color: SURFACE_STRIPE,
        code_bg_color: SURFACE_STRIPE,
        window_stroke: Stroke::new(BORDER_WIDTH, BORDER),

        weak_text_alpha: WEAK_TEXT_ALPHA,
        disabled_alpha: DISABLED_ALPHA,

        // egui's own light red and orange are under AA on white.
        error_fg_color: Color32::from_rgb(0x80, 0x00, 0x00),
        warn_fg_color: Color32::from_rgb(0x80, 0x52, 0x00),
        hyperlink_color: Color32::from_rgb(0x00, 0x47, 0x8c),

        ..Visuals::light()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use egui::{Margin, Vec2, style::WidgetVisuals};

    /// What `Style::button_style` lays out for one widget state, as `(allocated, painted)`.
    ///
    /// A plain `Button` is always `framed`; `Button::selectable` (and so `selectable_value` /
    /// `toggle_value` / `selectable_label`) drops the frame while unselected and un-hovered.
    fn button_box(padding: Vec2, widget: &WidgetVisuals, framed: bool) -> (Vec2, Vec2) {
        let border = widget.bg_stroke.width;
        let inner = Margin::from(padding + Vec2::splat(widget.expansion) - Vec2::splat(border));

        if framed {
            // outer_margin = -expansion.
            let painted = inner.sum() + Vec2::splat(2.0 * border);
            (painted - Vec2::splat(2.0 * widget.expansion), painted)
        } else {
            (inner.sum(), inner.sum())
        }
    }

    /// egui keeps a hovered button the same size by compensating the border out of the inner
    /// margin, which a non-zero idle border breaks: `Button::selectable`'s frameless branch keeps
    /// the reduced margin but paints no border. [`EXPANSION`] puts those pixels back.
    #[test]
    fn buttons_keep_their_size_in_every_state() {
        let style = high_contrast_style();
        let padding = style.spacing.button_padding;
        let widgets = &style.visuals.widgets;

        let cases = [
            ("idle", &widgets.inactive, true),
            (
                "idle, unframed (an unselected `selectable_value`)",
                &widgets.inactive,
                false,
            ),
            ("hovered", &widgets.hovered, true),
            ("focused", &widgets.active, true),
            ("open", &widgets.open, true),
            ("open, unframed", &widgets.open, false),
        ];

        let (expected, _) = button_box(padding, &widgets.inactive, true);

        for (name, widget, framed) in cases {
            let (allocated, _) = button_box(padding, widget, framed);
            assert_eq!(
                allocated, expected,
                "a {name} button takes {allocated:?} of space around its label, \
                 but an idle one takes {expected:?} - hovering it would move the UI",
            );
        }

        let (_, painted) = button_box(padding, &widgets.inactive, true);
        for (name, widget, framed) in cases.into_iter().filter(|(_, _, framed)| *framed) {
            let (_, drawn) = button_box(padding, widget, framed);
            assert_eq!(
                drawn, painted,
                "a {name} button paints a {drawn:?} box, an idle one paints {painted:?} - \
                 hovering it would visibly resize the button",
            );
        }
    }

    /// The compensation above only cancels exactly on whole pixels: `Margin` is stored as `i8`.
    #[test]
    fn every_border_width_lands_on_a_whole_pixel() {
        let style = high_contrast_style();
        let padding = style.spacing.button_padding;
        let widgets = &style.visuals.widgets;

        for (name, widget) in [
            ("idle", &widgets.inactive),
            ("hovered", &widgets.hovered),
            ("focused", &widgets.active),
            ("open", &widgets.open),
            ("noninteractive", &widgets.noninteractive),
        ] {
            let margin =
                padding + Vec2::splat(widget.expansion) - Vec2::splat(widget.bg_stroke.width);
            assert!(
                margin.x.fract() == 0.0 && margin.y.fract() == 0.0,
                "the inner margin of a {name} button is {margin:?}, which rounds when it becomes \
                 a `Margin` and leaves the button a pixel off",
            );
        }
    }
}
