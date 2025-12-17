use egui_tiles::SimplificationOptions;

use core::Core;

mod can;
mod commands;
mod horizon;
mod links;
mod map;
mod messages;
mod params;
mod plot;
mod sensors;
mod state_estimator;
mod status;

pub use can::CanProbePane;
pub use commands::CommandsPane;
pub use horizon::HorizonPane;
pub use links::LinksPane;
pub use map::MapPane;
pub use messages::MessagesPane;
pub use params::ParamsPane;
pub use plot::PlotPane;
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
    Placeholder(String),
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
            Pane::Map(p) => p.pane_ui(ui, self),
            Pane::Status(p) => p.pane_ui(ui, self),
            Pane::StateEstimator(p) => p.pane_ui(ui, self),
            Pane::Sensors(p) => p.pane_ui(ui, self),
            Pane::Plot(p) => p.pane_ui(ui, self),
            Pane::Messages(p) => p.pane_ui(ui, self),
            Pane::Commands(p) => p.pane_ui(ui, self),
            Pane::CanProbe(p) => p.pane_ui(ui, self),
            Pane::Links(p) => p.pane_ui(ui, self),
            Pane::Horizon(p) => p.pane_ui(ui, self),
            Pane::Params(p) => p.pane_ui(ui, self),
            Pane::Placeholder(_) => {}
        }

        egui_tiles::UiResponse::None
    }
}
