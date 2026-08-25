use egui::{Align, Layout};
use egui_tiles::SimplificationOptions;
use mavspec::rust::dialects::common::messages::{LocalPositionNed, VfrHud};

use nadir_core::{Source, System};

mod can;
mod commands;
mod horizon;
mod links;
mod logs;
mod map;
mod messages;
mod navigation;
mod params;
mod plot;
mod preflight;
mod propulsion;
mod sensors;
mod state_estimator;
mod status;

pub use can::CanProbePane;
pub use commands::CommandsPane;
pub use horizon::{HorizonPane, VelocityMode};
pub use links::LinksPane;
pub use logs::LogsPane;
pub use map::MapPane;
pub use messages::MessagesPane;
pub use navigation::NavigationPane;
pub use params::ParamsPane;
pub use plot::PlotPane;
pub use preflight::PreflightPane;
pub use propulsion::PropulsionPane;
pub use sensors::SensorsPane;
pub use state_estimator::StateEstimatorPane;
pub use status::StatusPane;

use crate::views::View;
use crate::widgets::SharedPlotState;

#[derive(Clone, Copy, PartialEq, Default)]
pub enum PositionSource {
    #[default]
    LocalPositionNed,
    VfrHud,
}

impl PositionSource {
    pub const ALL: [PositionSource; 2] = [PositionSource::LocalPositionNed, PositionSource::VfrHud];

    pub fn label(self) -> &'static str {
        match self {
            Self::LocalPositionNed => "Local",
            Self::VfrHud => "MSL (HUD)",
        }
    }

    pub fn has_data(self, source: &Source, system_id: u8) -> bool {
        let count = match self {
            Self::LocalPositionNed => source.db.message_count::<LocalPositionNed>(system_id, 1),
            Self::VfrHud => source.db.message_count::<VfrHud>(system_id, 1),
        };

        count > 0
    }
}

pub struct TreeBehavior<'a> {
    /// The source the active view names. Panes only ever read telemetry, so this is all they get:
    /// no links and no CAN proxy.
    pub source: Source,
    pub active_view: View,
    pub shared_plot_state: &'a mut SharedPlotState,
    pub position_source: &'a mut PositionSource,
}

// Links, Horizon and Status are not Pane variants: they live in the fixed status bar (see app.rs)
// rather than in the tile tree.
pub enum Pane {
    Map(Box<MapPane>),
    StateEstimator(StateEstimatorPane),
    Sensors(SensorsPane),
    Plot(PlotPane),
    Messages(MessagesPane),
    Commands(CommandsPane),
    CanProbe(CanProbePane),
    Params(ParamsPane),
    Propulsion(PropulsionPane),
    Preflight(PreflightPane),
    Navigation(NavigationPane),
    FlightLogs(LogsPane),
    #[cfg(feature = "profiling")]
    Profiler,
    Placeholder(String),
}

pub trait PaneUi {
    fn system_ui(&mut self, _ui: &mut egui::Ui, _system: System) {}

    fn inset(&mut self, _ui: &mut egui::Ui) -> f32 {
        5.0
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System { system_id, .. } = behavior.active_view else {
            return;
        };

        let Some(system) = behavior.source.system(system_id) else {
            return;
        };

        self.system_ui(ui, system);
    }

    fn outer_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let inset = self.inset(ui);
        let rect = ui.clip_rect();
        let inner_rect = rect.shrink(inset);
        if inner_rect.width() < 10.0 || inner_rect.height() < 10.0 {
            return;
        }

        ui.place(inner_rect, |ui: &mut egui::Ui| {
            let mut ui = egui::Ui::new(
                ui.ctx().clone(),
                ui.id().with("inner"),
                egui::UiBuilder::new()
                    .layer_id(ui.layer_id())
                    .layout(Layout::top_down(Align::LEFT))
                    .max_rect(inner_rect),
            );

            self.pane_ui(&mut ui, behavior);

            ui.response()
        });
    }
}

impl std::fmt::Display for Pane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name: String = match self {
            Pane::Map(_) => "Map".into(),
            Pane::StateEstimator(_) => "State Estimator".into(),
            Pane::Sensors(_) => "Sensors".into(),
            Pane::Plot(_) => "Plot".into(),
            Pane::Messages(_) => "Messages".into(),
            Pane::Commands(_) => "Commands".into(),
            Pane::CanProbe(_) => "CAN Probe".into(),
            Pane::Params(_) => "Params".into(),
            Pane::Propulsion(_) => "Propulsion".into(),
            Pane::Preflight(_) => "Preflight".into(),
            Pane::Navigation(_) => "Navigation".into(),
            Pane::FlightLogs(_) => "Flight Logs".into(),
            #[cfg(feature = "profiling")]
            Pane::Profiler => "Profiler".into(),
            Pane::Placeholder(s) => s.into(),
        };
        f.write_str(&name)
    }
}

impl egui_tiles::Behavior<Pane> for TreeBehavior<'_> {
    fn tab_bar_color(&self, visuals: &egui::Visuals) -> egui::Color32 {
        visuals
            .extreme_bg_color
            .lerp_to_gamma(visuals.faint_bg_color, 0.25)
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        30.0
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        2.0
    }

    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        SimplificationOptions {
            all_panes_must_have_tabs: true,
            join_nested_linear_containers: true,
            prune_empty_containers: true,
            prune_empty_tabs: true,
            prune_single_child_containers: true,
            prune_single_child_tabs: true,
            ..Default::default()
        }
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        format!("{pane}").into()
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut Pane,
    ) -> egui_tiles::UiResponse {
        #[cfg(feature = "profiling")]
        puffin::profile_function!(format!("{}", pane));

        match pane {
            Pane::Map(p) => p.outer_ui(ui, self),
            Pane::StateEstimator(p) => p.outer_ui(ui, self),
            Pane::Sensors(p) => p.outer_ui(ui, self),
            Pane::Plot(p) => p.outer_ui(ui, self),
            Pane::Messages(p) => p.outer_ui(ui, self),
            Pane::Commands(p) => p.outer_ui(ui, self),
            Pane::CanProbe(p) => p.outer_ui(ui, self),
            Pane::Params(p) => p.outer_ui(ui, self),
            Pane::Propulsion(p) => p.outer_ui(ui, self),
            Pane::Preflight(p) => p.outer_ui(ui, self),
            Pane::Navigation(p) => p.outer_ui(ui, self),
            Pane::FlightLogs(p) => p.outer_ui(ui, self),
            #[cfg(feature = "profiling")]
            Pane::Profiler => {
                puffin_egui::profiler_ui(ui);
            }
            Pane::Placeholder(_) => {
                ui.centered_and_justified(|ui| {
                    ui.weak("To be implemented.");
                });
            }
        }

        egui_tiles::UiResponse::None
    }
}
