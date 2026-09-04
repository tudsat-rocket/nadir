//! Window themes.
//!
//! Two of the three are egui's own; the third, [`Theme::HighContrast`], is the light theme retuned
//! against WCAG 2.2 level AA (and, through it, EN 301 549 clause 11, which is what applies to a
//! desktop application rather than a web page). The findings it answers are written up in
//! `docs/accessibility-review.md`.
//!
//! egui has no third theme slot, so high contrast is applied as a replacement *light* style: the
//! preference is pinned to light and `Options::light_style` is swapped out. Switching away puts
//! egui's own light style back, so nothing here leaks into the other two themes.

use egui::style::{Selection, WidgetVisuals, Widgets};
use egui::{Color32, CornerRadius, Stroke, Style, Visuals};

use nadir_core::settings::Theme;

use crate::colors;

/// Every surface a control can sit on is white or near-white, and every control is identified by a
/// border rather than by a fill. That is the usual shape of a high-contrast theme, and it is also
/// what makes SC 1.4.11 pass: egui's light theme separates a button from its panel by 1.18:1,
/// which is not a boundary so much as a rumour.
const SURFACE: Color32 = Color32::WHITE;
/// Text edits, the arm/caution strip, instrument boxes - anything "inset" into a panel.
const SURFACE_INSET: Color32 = Color32::from_gray(237);
/// Striped table rows.
const SURFACE_STRIPE: Color32 = Color32::from_gray(232);

const BORDER: Color32 = Color32::from_gray(26);
/// Separators and indentation lines. Lighter than [`BORDER`] so a table of them does not read as a
/// grid of controls, but still 6.9:1 against the panel.
const BORDER_WEAK: Color32 = Color32::from_gray(90);

const FILL_HOVERED: Color32 = Color32::from_gray(214);
const FILL_ACTIVE: Color32 = Color32::from_gray(184);

/// WCAG 2.2 SC 2.4.11 wants a focus indicator at least 2 px thick, and egui derives the focused
/// look from `widgets.active`.
const FOCUS_WIDTH: f32 = 2.0;
/// The resting border. Thinner than the focus ring, so gaining focus reads as a change.
const BORDER_WIDTH: f32 = 1.0;

/// Every widget state expands by the same amount, and that amount equals [`BORDER_WIDTH`]. Both
/// halves of that matter, and neither is cosmetic - see `buttons_keep_their_size_in_every_state`.
///
/// `Style::button_style` sizes a button as `content + 2*(button_padding + expansion - border) +
/// 2*border - 2*expansion`. The border terms cancel, so a button only keeps its size across states
/// while the inner margin lands on whole pixels (`Margin` is `i8` and the conversion rounds). A
/// uniform expansion additionally keeps the *painted* rectangle the same size in every state.
///
/// The second half is `Button::selectable`, which drops the frame entirely while unselected: that
/// path keeps the reduced inner margin but paints no border, so it comes out `2*border` too small
/// unless the expansion puts those pixels back. egui's own themes get this for free by leaving the
/// idle border at zero width; a theme that draws one has to pay for it here.
const EXPANSION: f32 = BORDER_WIDTH;

/// Selected buttons fill with this and label themselves with [`SELECTION_FG`]; the same fill is the
/// text-selection highlight in a `TextEdit`, where the text stays the ordinary body colour. It
/// therefore has to stay light enough for black text (13.5:1) while still carrying the dark blue
/// (9.5:1).
const SELECTION_BG: Color32 = Color32::from_rgb(0xa8, 0xd4, 0xff);
const SELECTION_FG: Color32 = Color32::from_rgb(0x00, 0x2a, 0x45);

/// Body text is pure black on white, so 0.6 (egui's default) would still clear AA. 0.7 keeps a
/// visible weight difference with room to spare for the further [`colors::dim`] fade on top.
const WEAK_TEXT_ALPHA: f32 = 0.7;
/// Disabled controls are exempt from the contrast criteria, but 0.5 is hard to read even when you
/// only want to know what the control says.
const DISABLED_ALPHA: f32 = 0.6;

/// Nothing in this UI is "large text", so 24 px is the SC 2.5.8 target-size minimum for every
/// control. egui defaults to 18. A touch screen wants more than the web minimum.
const MIN_CONTROL_HEIGHT: f32 = 24.0;
const MIN_CONTROL_HEIGHT_TOUCH: f32 = 32.0;

/// Applies `theme` to `ctx`, and tells [`colors`] which palette the rest of the frame should use.
pub fn apply(ctx: &egui::Context, theme: Theme) {
    colors::set_high_contrast(theme == Theme::HighContrast);

    // Rebuilt from scratch every time rather than mutated in place, so switching *out* of high
    // contrast restores egui's light theme instead of leaving half the overrides behind.
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
        // High contrast is a light style, so it must not be left following a dark desktop.
        Theme::Light | Theme::HighContrast => egui::ThemePreference::Light,
    });
}

/// The label to show in the preferences.
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

    // A 2 px border eats into a button's inner margin, so the padding has to grow with it or the
    // label ends up touching the frame. Whole pixels, so the margin arithmetic in
    // `Style::button_style` stays exact - see [`EXPANSION`].
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.interact_size.y = if cfg!(target_os = "android") {
        MIN_CONTROL_HEIGHT_TOUCH
    } else {
        MIN_CONTROL_HEIGHT
    };

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
            // Also the focused state: egui picks `active` for a widget that has keyboard focus.
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

        // egui's own light values are 4.0:1 (its red) and 3.5:1 (its orange) against white. Match
        // what `colors::readable` does to the indicator palette under this theme.
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
    /// `framed` is the branch egui picks: a plain `Button` is always framed, but
    /// `Button::selectable` (and so `Ui::selectable_value` / `toggle_value` / `selectable_label`)
    /// drops the frame while it is both unselected and un-hovered.
    fn button_box(padding: Vec2, widget: &WidgetVisuals, framed: bool) -> (Vec2, Vec2) {
        let border = widget.bg_stroke.width;
        let inner = Margin::from(padding + Vec2::splat(widget.expansion) - Vec2::splat(border));

        if framed {
            // Frame: content + inner_margin + 2*stroke + outer_margin, outer_margin = -expansion.
            let painted = inner.sum() + Vec2::splat(2.0 * border);
            (painted - Vec2::splat(2.0 * widget.expansion), painted)
        } else {
            // `Frame::new().inner_margin(frame.inner_margin)` - the margin, and nothing else.
            (inner.sum(), inner.sum())
        }
    }

    /// Hovering a button must not resize it, or every widget after it on the row shifts. egui keeps
    /// this true by compensating the border out of the inner margin, which a theme can break in two
    /// ways at once: a fractional border width that does not survive the `i8` `Margin`, and a
    /// non-zero idle border that `Button::selectable`'s frameless branch never paints.
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

        // The frameless branch paints no border, so only compare the states that draw one.
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

    /// The compensation only cancels exactly on whole pixels, because `Margin` is stored as `i8`.
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
