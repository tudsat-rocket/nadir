mod map;
mod messages;
mod plot;
mod sensors;
mod status;

use core::Core;

pub use map::MapPane;
pub use messages::MessagesPane;
pub use plot::PlotPane;
pub use sensors::SensorsPane;
pub use status::StatusPane;

use crate::views::View;
use crate::widgets::SharedPlotState;

pub struct TreeBehavior<'a> {
    pub core: Core,
    pub active_view: View,
    pub shared_plot_state: &'a mut SharedPlotState,
}

pub enum Pane {
    Map(MapPane),
    Status(StatusPane),
    Sensors(SensorsPane),
    Plot(PlotPane),
    Messages(MessagesPane),
    Placeholder(String),
}

impl<'a> egui_tiles::Behavior<Pane> for TreeBehavior<'a> {
    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        match pane {
            Pane::Map(_) => "Map".into(),
            Pane::Status(_) => "Status".into(),
            Pane::Sensors(_) => "Sensors".into(),
            Pane::Plot(_) => "Plot".into(),
            Pane::Messages(_) => "MAVLink Messages".into(),
            Pane::Placeholder(s) => s.into(),
        }
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut Pane,
    ) -> egui_tiles::UiResponse {
        match pane {
            Pane::Map(p) => p.pane_ui(ui, self),
            Pane::Status(p) => p.pane_ui(ui, self),
            Pane::Sensors(p) => p.pane_ui(ui, self),
            Pane::Plot(p) => p.pane_ui(ui, self),
            Pane::Messages(p) => p.pane_ui(ui, self),
            Pane::Placeholder(_) => {}
        }

        egui_tiles::UiResponse::None
    }
}
