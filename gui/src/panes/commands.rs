use core::System;

use eframe::egui;
use egui::{Align, Layout};

use crate::panes::PaneUi;

pub struct CommandsPane {}

impl CommandsPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }
}

impl PaneUi for CommandsPane {
    fn system_ui(&mut self, ui: &mut egui::Ui, _system: System) {
        use egui_extras::{Column, TableBuilder};
        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
            ui.horizontal(|ui| {
                ui.label("schinken");
            });

            TableBuilder::new(ui)
                .column(Column::auto().resizable(true))
                .column(Column::auto().resizable(true))
                .column(Column::auto().resizable(true))
                .column(Column::auto().resizable(true))
                .column(Column::auto().resizable(true))
                .column(Column::auto().resizable(true))
                .column(Column::auto().resizable(true))
                .column(Column::auto().resizable(true))
                .column(Column::auto().resizable(true))
                .column(Column::remainder())
                //.header(20.0, |mut header| {
                //    header.col(|ui| {
                //        ui.heading("First column");
                //    });
                //    header.col(|ui| {
                //        ui.heading("Second column");
                //    });
                //})
                .body(|mut body| {
                    body.row(20.0, |mut row| {
                        row.col(|ui| {
                            ui.weak("Sent At");
                        });
                        row.col(|ui| {
                            ui.weak("Target Component");
                        });
                        row.col(|ui| {
                            ui.weak("Message");
                        });
                        row.col(|ui| {
                            ui.weak("P1");
                        });
                        row.col(|ui| {
                            ui.weak("P2");
                        });
                        row.col(|ui| {
                            ui.weak("P3");
                        });
                        row.col(|ui| {
                            ui.weak("P4");
                        });
                        row.col(|ui| {
                            ui.weak("P5");
                        });
                        row.col(|ui| {
                            ui.weak("P6");
                        });
                        row.col(|ui| {
                            ui.weak("P7");
                        });
                    });
                    body.row(20.0, |mut row| {
                        row.col(|ui| {
                            ui.label("16:10:32");
                        });
                        row.col(|ui| {
                            ui.monospace("0x01");
                        });
                        row.col(|ui| {
                            ui.monospace("MAV_CMD_NAV_WAYPOINT");
                        });
                        row.col(|ui| {
                            ui.label("P1");
                        });
                        row.col(|ui| {
                            ui.label("P2");
                        });
                        row.col(|ui| {
                            ui.label("P3");
                        });
                        row.col(|ui| {
                            ui.label("P4");
                        });
                        row.col(|ui| {
                            ui.label("P5");
                        });
                        row.col(|ui| {
                            ui.label("P6");
                        });
                        row.col(|ui| {
                            ui.label("P7");
                        });
                    });
                });
        });
    }
}
