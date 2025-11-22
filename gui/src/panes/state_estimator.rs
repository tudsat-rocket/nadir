use crate::panes::TreeBehavior;
use crate::views::View;
use crate::widgets::{Plot, PlotLine};

pub struct StateEstimatorPane {}

impl StateEstimatorPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };

        let size = egui::Vec2::new(ui.available_width(), ui.available_height() * 0.33);

        let altitude_plot = Plot::new(
            vec![PlotLine {
                system_id: system_id,
                component_id: 1,
                message_name: "LOCAL_POSITION_NED".to_owned(),
                field_name: "z".to_owned(),
                alias: None,
                unit: None,
                color: None,
            }],
            &behavior.core,
            &mut behavior.shared_plot_state,
            (None, None),
        );
        ui.add_sized(size, altitude_plot);

        let velocity_plot = Plot::new(
            vec![PlotLine {
                system_id: system_id,
                component_id: 1,
                message_name: "LOCAL_POSITION_NED".to_owned(),
                field_name: "vz".to_owned(),
                alias: None,
                unit: None,
                color: None,
            }],
            &behavior.core,
            &mut behavior.shared_plot_state,
            (None, None),
        );
        ui.add_sized(size, velocity_plot);
    }
}
