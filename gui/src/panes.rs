use egui::{Align, Layout};
use egui_tiles::SimplificationOptions;

use core::{Core, System};

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
pub use horizon::HorizonPane;
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

pub struct TreeBehavior<'a> {
    pub core: Core,
    pub active_view: View,
    pub shared_plot_state: &'a mut SharedPlotState,
}

pub enum Pane {
    Map(Box<MapPane>),
    Status(StatusPane),
    StateEstimator(StateEstimatorPane),
    Sensors(SensorsPane),
    Plot(PlotPane),
    Messages(MessagesPane),
    Commands(CommandsPane),
    CanProbe(CanProbePane),
    Links(LinksPane),
    Horizon(HorizonPane),
    Params(ParamsPane),
    Propulsion(PropulsionPane),
    Preflight(PreflightPane),
    Navigation(NavigationPane),
    FlightLogs(LogsPane),
    Placeholder(String),
}

pub trait PaneUi {
    fn system_ui(&mut self, _ui: &mut egui::Ui, _system: System) {}

    fn inset(&mut self, _ui: &mut egui::Ui) -> f32 {
        5.0
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };

        let Some(system) = behavior.core.system(system_id) else {
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
            Pane::Status(_) => "Status".into(),
            Pane::StateEstimator(_) => "State Estimator".into(),
            Pane::Sensors(_) => "Sensors".into(),
            Pane::Plot(_) => "Plot".into(),
            Pane::Messages(_) => "Messages".into(),
            Pane::Commands(_) => "Commands".into(),
            Pane::CanProbe(_) => "CAN Probe".into(),
            Pane::Links(_) => "Link".into(),
            Pane::Horizon(_) => "Horizon".into(),
            Pane::Params(_) => "Params".into(),
            Pane::Propulsion(_) => "Propulsion".into(),
            Pane::Preflight(_) => "Preflight".into(),
            Pane::Navigation(_) => "Navigation".into(),
            Pane::FlightLogs(_) => "Flight Logs".into(),
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
            //..SimplificationOptions::OFF
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
            Pane::Status(p) => p.outer_ui(ui, self),
            Pane::StateEstimator(p) => p.outer_ui(ui, self),
            Pane::Sensors(p) => p.outer_ui(ui, self),
            Pane::Plot(p) => p.outer_ui(ui, self),
            Pane::Messages(p) => p.outer_ui(ui, self),
            Pane::Commands(p) => p.outer_ui(ui, self),
            Pane::CanProbe(p) => p.outer_ui(ui, self),
            Pane::Links(p) => p.outer_ui(ui, self),
            Pane::Horizon(p) => p.outer_ui(ui, self),
            Pane::Params(p) => p.outer_ui(ui, self),
            Pane::Propulsion(p) => p.outer_ui(ui, self),
            Pane::Preflight(p) => p.outer_ui(ui, self),
            Pane::Navigation(p) => p.outer_ui(ui, self),
            Pane::FlightLogs(p) => p.outer_ui(ui, self),
            Pane::Placeholder(_) => {
                ui.centered_and_justified(|ui| {
                    ui.weak("To be implemented.");
                });
            }
        }

        egui_tiles::UiResponse::None
    }
}
