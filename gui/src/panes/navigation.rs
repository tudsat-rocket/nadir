use egui::Grid;
use mavspec::rust::dialects::common::messages::PositionTargetGlobalInt;

use crate::panes::TreeBehavior;
use crate::views::View;

pub struct NavigationPane {}

impl NavigationPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };

        let Some(system) = behavior.core.system(system_id) else {
            return;
        };

        let w = ui.available_width();

        ui.horizontal(|ui| {
            ui.add_space(10.0);

            ui.vertical(|ui| {
                ui.add_space(10.0);

                ui.add_space(5.0);
                ui.weak("🗺 Terrain Data");
                ui.add_space(5.0);
                ui.label("TODO");
                ui.add_space(50.0);

                ui.separator();

                ui.add_space(5.0);
                ui.weak("🏁 Target Data");
                ui.add_space(5.0);
                if let Some(target) = system.last_target_global_int().ok().flatten() {
                    let PositionTargetGlobalInt {
                        time_boot_ms: _,
                        coordinate_frame,
                        type_mask,
                        lat_int: _,
                        lon_int: _,
                        alt,
                        vx,
                        vy,
                        vz,
                        afx,
                        afy,
                        afz,
                        yaw,
                        yaw_rate,
                    } = target;

                    Grid::new(ui.next_auto_id())
                        .num_columns(4)
                        .min_col_width(w / 4.0)
                        .show(ui, |ui| {
                            ui.set_width(w);

                            ui.weak("Frame");
                            ui.label(format!("{coordinate_frame:?}"));
                            ui.weak("Type Mask");
                            ui.label(format!("0x{:04x}", type_mask.bits()));
                            ui.end_row();

                            ui.weak("Altitude");
                            ui.label(format!("{alt:.1}m"));
                            ui.weak("Yaw (rate)");
                            ui.label(format!("{yaw:.1} ({yaw_rate:.1})"));
                            ui.end_row();

                            ui.weak("Velocity");
                            ui.label(format!("({vx}, {vy}, {vz})"));
                            ui.weak("Acc.");
                            ui.label(format!("({afx}, {afy}, {afz})"));
                            ui.end_row();
                        });
                } else {
                    ui.weak("No target information.");
                }
                ui.add_space(5.0);

                ui.separator();

                ui.add_space(5.0);
                ui.weak("🏅 Flight Records");
                ui.add_space(5.0);
                Grid::new(ui.next_auto_id())
                    .num_columns(4)
                    .min_col_width(w / 4.0)
                    .show(ui, |ui| {
                        ui.set_width(w);

                        ui.weak("Flight Time");
                        ui.label("1:42:23");
                        ui.weak("Distance over Ground");
                        ui.label("23.1 km");
                        ui.end_row();

                        ui.weak("Max. Downrange");
                        ui.label("16 km");
                        ui.weak("Max. Altitude / Apogee");
                        ui.label("110.1 m");
                        ui.end_row();

                        ui.weak("Max. Airspeed");
                        ui.label("100 m/s");
                        ui.weak("Max. Groundspeed");
                        ui.label("60 m/s");
                        ui.end_row();

                        ui.weak("Max. Acceleration");
                        ui.label("10 m/s^2");
                        ui.weak("TODO");
                        ui.label("");
                        ui.end_row();
                    });
                ui.add_space(5.0);
            });
        });
    }
}
