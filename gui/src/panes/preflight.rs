use core::ParamProgress;

use egui::Vec2;
use mavspec::rust::dialects::common::enums::{
    MavProtocolCapability, MavSysStatusSensor, MavSysStatusSensorExtended,
};
use mavspec::rust::dialects::common::messages::SysStatus;

use crate::colors::{COLOR_INDICATOR_GOOD, COLOR_INDICATOR_WARNING};
use crate::panes::TreeBehavior;
use crate::views::View;

pub struct PreflightPane {}

impl PreflightPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }

    fn sys_status_checks_ui(&mut self, ui: &mut egui::Ui, sys_status: &SysStatus) {
        let num_unhealthy_basic = MavSysStatusSensor::all()
            .iter()
            .filter(|stat| {
                sys_status.onboard_control_sensors_present.contains(*stat)
                    && sys_status.onboard_control_sensors_enabled.contains(*stat)
                    && !sys_status.onboard_control_sensors_health.contains(*stat)
            })
            .count();
        let num_disabled_basic = MavSysStatusSensor::all()
            .iter()
            .filter(|stat| {
                sys_status.onboard_control_sensors_present.contains(*stat)
                    && !sys_status.onboard_control_sensors_enabled.contains(*stat)
            })
            .count();
        let num_unhealthy_extended = MavSysStatusSensorExtended::all()
            .iter()
            .filter(|stat| {
                sys_status
                    .onboard_control_sensors_present_extended
                    .contains(*stat)
                    && sys_status
                        .onboard_control_sensors_enabled_extended
                        .contains(*stat)
                    && !sys_status
                        .onboard_control_sensors_health_extended
                        .contains(*stat)
            })
            .count();
        let num_disabled_extended = MavSysStatusSensorExtended::all()
            .iter()
            .filter(|stat| {
                sys_status
                    .onboard_control_sensors_present_extended
                    .contains(*stat)
                    && !sys_status
                        .onboard_control_sensors_enabled_extended
                        .contains(*stat)
            })
            .count();

        let num_unhealthy = num_unhealthy_basic + num_unhealthy_extended;
        let num_disabled = num_disabled_basic + num_disabled_extended;

        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.weak("🚨 System Status Checks");
            if num_unhealthy > 0 || num_disabled > 0 {
                if num_unhealthy > 0 {
                    ui.colored_label(COLOR_INDICATOR_WARNING, format!("⚠ {num_unhealthy}"));
                }
                if num_disabled > 0 {
                    ui.weak(format!("💤 {num_disabled}"));
                }
            } else {
                ui.colored_label(COLOR_INDICATOR_GOOD, "✔");
            }
        });
        ui.add_space(5.0);

        egui::ScrollArea::vertical()
            .max_width(ui.available_width())
            .show(ui, |ui| {
                egui::Grid::new(ui.next_auto_id())
                    .num_columns(2)
                    .striped(true)
                    .spacing(Vec2::new(50.0, ui.spacing().item_spacing.y))
                    .show(ui, |ui| {
                        let mut extensions_used = false;

                        for (name, stat) in MavSysStatusSensor::all().iter_names() {
                            if stat == MavSysStatusSensor::MAV_SYS_STATUS_EXTENSION_USED {
                                extensions_used = true;
                                continue;
                            }

                            let present = sys_status.onboard_control_sensors_present.contains(stat);

                            if !present {
                                continue;
                            }

                            let enabled = sys_status.onboard_control_sensors_enabled.contains(stat);
                            let healthy = sys_status.onboard_control_sensors_health.contains(stat);

                            let name_display = name
                                .replace("MAV_SYS_STATUS_", "")
                                .replace("_3D_", "")
                                .replace('_', " ");

                            ui.weak(name_display);

                            if !enabled {
                                ui.weak("💤 Disabled");
                            } else if healthy {
                                ui.colored_label(COLOR_INDICATOR_GOOD, "✔");
                            } else {
                                ui.colored_label(COLOR_INDICATOR_WARNING, "⚠ Unhealthy");
                            }

                            ui.end_row();
                        }

                        if !extensions_used {
                            return;
                        }

                        for (name, stat) in MavSysStatusSensorExtended::all().iter_names() {
                            let present = sys_status
                                .onboard_control_sensors_present_extended
                                .contains(stat);

                            if !present {
                                continue;
                            }

                            let enabled = sys_status
                                .onboard_control_sensors_enabled_extended
                                .contains(stat);
                            let healthy = sys_status
                                .onboard_control_sensors_health_extended
                                .contains(stat);

                            let name_display =
                                name.replace("MAV_SYS_STATUS_", "").replace('_', " ");

                            ui.weak(name_display);

                            if !enabled {
                                ui.weak("💤 Disabled");
                            } else if healthy {
                                ui.colored_label(COLOR_INDICATOR_GOOD, "✔");
                            } else {
                                ui.colored_label(COLOR_INDICATOR_WARNING, "⚠ Unhealthy");
                            }

                            ui.end_row();
                        }
                    });
            });
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };

        let Some(system) = behavior.core.system(system_id) else {
            return;
        };

        let sys_status = system.last_sys_status().ok().flatten();
        let autopilot_version = system.last_autopilot_version().ok().flatten();
        let capabilities =
            autopilot_version.map_or(MavProtocolCapability::empty(), |av| av.capabilities);

        let w = f32::max(20.0, ui.available_width() - 10.0);

        if w <= 20.0 {
            return;
        }

        ui.add_space(10.0);

        ui.horizontal_centered(|ui| {
            ui.add_space(10.0);

            ui.vertical(|ui| {
                if let Some(ss) = sys_status.as_ref() {
                    self.sys_status_checks_ui(ui, ss);
                } else {
                    ui.add_space(5.0);
                    ui.weak("🚨 System Status Checks");
                    ui.add_space(5.0);

                    ui.set_width(ui.available_width() * 0.4);
                    ui.centered_and_justified(|ui| {
                        ui.weak("Not available.");
                    });
                }
            });

            ui.separator();

            ui.vertical(|ui| {
                ui.add_space(5.0);
                ui.weak("📈 Error Counts (Comm, 1, 2, 3, 4)");
                ui.add_space(5.0);

                if let Some(ss) = sys_status.as_ref() {
                    ui.horizontal(|ui| {
                        ui.monospace(format!("{}", ss.errors_comm));
                        ui.monospace(format!("{}", ss.errors_count1));
                        ui.monospace(format!("{}", ss.errors_count2));
                        ui.monospace(format!("{}", ss.errors_count3));
                        ui.monospace(format!("{}", ss.errors_count4));
                    });
                } else {
                    ui.weak("Not available.");
                }

                ui.add_space(5.0);

                ui.separator();

                ui.add_space(5.0);
                ui.weak("🕹 Ground Station Checks");
                ui.add_space(5.0);

                egui::Grid::new(ui.next_auto_id())
                    .num_columns(2)
                    .min_col_width(ui.available_width() / 2.0)
                    .striped(true)
                    .show(ui, |ui| {
                        let params_supported = !(capabilities
                            & (MavProtocolCapability::PARAM_FLOAT
                                | MavProtocolCapability::PARAM_ENCODE_C_CAST
                                | MavProtocolCapability::PARAM_ENCODE_BYTEWISE))
                            .is_empty();
                        let terrain_supported =
                            !(capabilities & MavProtocolCapability::TERRAIN).is_empty();
                        let mission_supported =
                            !(capabilities & MavProtocolCapability::MISSION_INT).is_empty();

                        ui.weak("Parameters");
                        if params_supported {
                            let params = system.params.lock().unwrap();
                            match &*params {
                                ParamProgress::Unknown => {
                                    ui.colored_label(COLOR_INDICATOR_WARNING, "⚠ Unknown");
                                }
                                ParamProgress::Progress(i, count) => {
                                    let pb = egui::ProgressBar::new((*i as f32) / (*count as f32))
                                        .desired_width(ui.available_width())
                                        .text(format!("{i}/{count}"));
                                    ui.add(pb);
                                }
                                ParamProgress::Failed(res) => {
                                    ui.colored_label(
                                        COLOR_INDICATOR_WARNING,
                                        format!("⚠ Failed: {res:?}"),
                                    );
                                }
                                ParamProgress::Complete(params) => {
                                    ui.horizontal(|ui| {
                                        ui.colored_label(COLOR_INDICATOR_GOOD, "✔");
                                        ui.weak(format!("{} params", params.len()));
                                    });
                                }
                            }
                        } else {
                            ui.weak("Not Supported");
                        }
                        ui.end_row();

                        ui.weak("Mission");
                        if mission_supported {
                            ui.colored_label(COLOR_INDICATOR_WARNING, "⚠ To be implemented");
                        } else {
                            ui.weak("Not Supported");
                        }
                        ui.end_row();

                        ui.weak("Terrain");
                        if terrain_supported {
                            ui.colored_label(COLOR_INDICATOR_WARNING, "⚠ To be implemented");
                        } else {
                            ui.weak("Not Supported");
                        }
                        ui.end_row();
                    });
            });
        });
    }
}
