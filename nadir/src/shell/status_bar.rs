use nadir_core::System;

use eframe::egui;
use egui::{Align, Layout, Pos2, Rect, Vec2};

use crate::panes::{HorizonPane, LinksPane, PaneUi as _, StatusPane, TreeBehavior};
mod vitals;
use vitals::Vitals;

/// The bar is fixed furniture: its height is a design constant, and the horizon flexes with the
/// leftover width between hard aspect limits so it never degenerates into a sliver or a panorama.
const HEIGHT: f32 = 160.0;
const HORIZON_MIN_ASPECT: f32 = 1.5;
const HORIZON_MAX_ASPECT: f32 = 2.6;
/// Below this the horizon is too small to read an attitude off, so the zone goes to the links.
const HORIZON_MIN_WIDTH: f32 = 100.0;
/// Below this the vitals columns are dropped entirely.
const VITALS_MIN_WIDTH: f32 = 150.0;
/// Gap between zones; the separators are painted down its middle.
const GAP: f32 = 6.0;

/// Carves a zone out of the bar. `None` for a zone too narrow to draw into at all.
fn zone(ui: &mut egui::Ui, id: &str, rect: Rect) -> Option<egui::Ui> {
    (rect.width() >= 10.0).then(|| {
        ui.new_child(
            egui::UiBuilder::new()
                .id_salt(id)
                .max_rect(rect)
                .layout(Layout::top_down(Align::LEFT)),
        )
    })
}

/// Always-visible strip above the tile tree, carrying what a pilot needs at all times: link state,
/// attitude, the arm controls with their alert lines, and the vitals columns.
///
/// It owns the panes it hosts, which are deliberately the same types the tile tree uses, so a zone
/// and a tile render identically.
pub struct StatusBar {
    status: StatusPane,
    horizon: HorizonPane,
    links: LinksPane,
}

impl StatusBar {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            status: StatusPane::new(ctx),
            horizon: HorizonPane::new(ctx),
            links: LinksPane::new(ctx),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, system: &System, behavior: &mut TreeBehavior<'_>) {
        egui::Panel::top("status_bar")
            .exact_size(HEIGHT)
            .frame(egui::Frame::new().fill(ui.ctx().global_style().visuals.window_fill()))
            .show_inside(ui, |ui| {
                let rect = ui.max_rect();

                // The mode panel is anchored to the true center of the bar so its buttons keep their
                // screen position no matter what the side panels are doing. On narrow windows (half
                // a screen with a tiling WM) the sides shed content instead of breaking: links go
                // first, then the horizon, and the right cluster collapses to its first column.
                let center_min = 420.0_f32.min(rect.width() - 2.0 * GAP);
                let center_w = (rect.width() * 0.38).clamp(center_min, 860.0);
                let center_rect =
                    Rect::from_center_size(rect.center(), Vec2::new(center_w, rect.height()));
                let side_w = (rect.width() - center_w) / 2.0 - GAP;

                let horizon_min = rect.height() * HORIZON_MIN_ASPECT;
                let horizon_max = rect.height() * HORIZON_MAX_ASPECT;
                // Preferred horizon width, tied to the overall window width so it keeps scaling down
                // instead of dominating half-width windows; the links zone absorbs the remainder.
                let horizon_pref = (rect.width() * 0.16).clamp(horizon_min, horizon_max);

                // The left zone is shared between the horizon and the links pane, with the horizon
                // taking priority: it keeps its minimum width, links take the rest and switch to
                // their compact form on the way down. Once even that no longer fits, links are
                // dropped and the horizon spans the whole zone - the uplink and downlink rows in the
                // consumables column then carry the link state.
                let (horizon_w, links_w) =
                    if side_w >= horizon_min + GAP + LinksPane::COMPACT_MIN_WIDTH {
                        let horizon_w = (side_w - GAP - LinksPane::FULL_MIN_WIDTH)
                            .clamp(horizon_min, horizon_pref);
                        (horizon_w, side_w - GAP - horizon_w)
                    } else {
                        (side_w, 0.0)
                    };
                let show_links = links_w >= LinksPane::COMPACT_MIN_WIDTH;
                let show_horizon = horizon_w >= HORIZON_MIN_WIDTH;

                let horizon_rect = Rect::from_min_max(
                    Pos2::new(center_rect.min.x - GAP - horizon_w, rect.min.y),
                    Pos2::new(center_rect.min.x - GAP, rect.max.y),
                );
                let links_rect =
                    Rect::from_min_max(rect.min, Pos2::new(horizon_rect.min.x - GAP, rect.max.y));
                let right_rect =
                    Rect::from_min_max(Pos2::new(center_rect.max.x + GAP, rect.min.y), rect.max);
                let show_right = side_w >= VITALS_MIN_WIDTH;

                if show_links && let Some(mut zui) = zone(ui, "bar_links", links_rect) {
                    self.links.system_ui(&mut zui, system.clone());
                }
                if show_horizon && let Some(mut zui) = zone(ui, "bar_horizon", horizon_rect) {
                    self.horizon.pane_ui(&mut zui, behavior);
                }
                if let Some(mut zui) = zone(ui, "bar_status", center_rect) {
                    self.status.system_ui(&mut zui, system.clone());
                }
                if show_right && let Some(mut zui) = zone(ui, "bar_vitals", right_rect) {
                    zui.add(Vitals {
                        system,
                        compact: side_w < Vitals::FULL_MIN_WIDTH,
                    });
                }

                let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
                if show_links {
                    ui.painter()
                        .vline(horizon_rect.min.x - GAP / 2.0, rect.y_range(), stroke);
                }
                if show_horizon {
                    ui.painter()
                        .vline(center_rect.min.x - GAP / 2.0, rect.y_range(), stroke);
                }
                if show_right {
                    ui.painter()
                        .vline(center_rect.max.x + GAP / 2.0, rect.y_range(), stroke);
                }
            });
    }
}
