use std::time::Duration;

use async_channel::{Receiver, Sender};
use egui::{Align, Key, Layout, Margin};
use egui_tiles::LinearDir;
use mavspec::rust::dialects::common::enums::MavResult;
use mavspec::rust::dialects::common::messages::CommandAck;
use socketcan::CanFrame;

use crate::panes::*;
use crate::views::View;
use crate::widgets::SharedPlotState;

pub struct App {
    core: core::Core,
    ack_rx: std::sync::mpsc::Receiver<CommandAck>,
    log_collector: egui_tracing::tracing::collector::EventCollector,
    toasts: egui_notify::Toasts,
    tiles_tree: egui_tiles::Tree<Pane>,
    shared_plot_state: SharedPlotState,
    active_view: View,
    sidebar_collapsed: bool,
}

impl App {
    pub fn new(
        log_collector: egui_tracing::tracing::collector::EventCollector,
        ctx: &egui::Context,
    ) -> Self {
        let (ack_tx, ack_rx) = std::sync::mpsc::channel::<CommandAck>();
        let ctx2 = ctx.clone();

        let (tx_to_proxy, rx_to_proxy) = async_channel::bounded::<CanFrame>(16);
        let (tx_from_proxy, rx_from_proxy) = async_channel::bounded::<CanFrame>(16);

        let core = core::Core::init()
            .on_ack(Box::new(move |ack| {
                ack_tx.send(ack.clone()).unwrap();
            }))
            .on_event(Box::new(move |_event| {
                ctx2.request_repaint();
            }))
            .set_can_proxy(tx_to_proxy, rx_from_proxy);

        let c = core.clone();
        std::thread::spawn(|| {
            let rt = tokio::runtime::Runtime::new().unwrap();

            // can socket
            let co = c.clone();
            rt.spawn(async move {
                // NOTE: ignore errors for now
                let _ = core::can_proxy::spawn_can_proxy(rx_to_proxy, tx_from_proxy, co).await;
            });

            rt.block_on(c.run());

            // tokio::runtime::Runtime::new().unwrap().block_on(c.run())
        });

        let mut tiles = egui_tiles::Tiles::default();

        let map = tiles.insert_pane(Pane::Map(MapPane::new(ctx, None)));

        let status = tiles.insert_pane(Pane::Status(StatusPane::new(ctx)));
        let components = tiles.insert_pane(Pane::Placeholder("Info".to_owned()));
        let top_left_tabs = tiles.insert_tab_tile(vec![status, components]);

        let link = tiles.insert_pane(Pane::Placeholder("Link".to_owned()));
        let cameras = tiles.insert_pane(Pane::Placeholder("Cameras".to_owned()));
        let top_right_tabs = tiles.insert_tab_tile(vec![link, cameras]);

        let system = tiles.insert_pane(Pane::Placeholder("System Overview".to_owned()));
        let state = tiles.insert_pane(Pane::Placeholder("State Estimate".to_owned()));
        let sensors = tiles.insert_pane(Pane::Sensors(SensorsPane::new(ctx)));
        let plot = tiles.insert_pane(Pane::Plot(PlotPane::new(ctx)));
        let messages = tiles.insert_pane(Pane::Messages(MessagesPane::new(ctx)));
        let commands = tiles.insert_pane(Pane::Commands(CommandsPane::new(ctx)));
        let can = tiles.insert_pane(Pane::CanProbePane(CanProbePane::new(ctx)));
        let bottom_tabs =
            tiles.insert_tab_tile(vec![system, state, sensors, plot, messages, commands, can]);

        let top_split = tiles.insert_new(egui_tiles::Tile::Container(
            egui_tiles::Container::Linear(egui_tiles::Linear::new_binary(
                LinearDir::Horizontal,
                [top_left_tabs, top_right_tabs],
                0.5,
            )),
        ));

        let side = tiles.insert_new(egui_tiles::Tile::Container(egui_tiles::Container::Linear(
            egui_tiles::Linear::new_binary(LinearDir::Vertical, [top_split, bottom_tabs], 0.33),
        )));

        let root = tiles.insert_new(egui_tiles::Tile::Container(egui_tiles::Container::Linear(
            egui_tiles::Linear::new_binary(LinearDir::Horizontal, [map, side], 0.45),
        )));

        let tiles_tree = egui_tiles::Tree::new("my_tree", root, tiles);

        Self {
            core,
            ack_rx,
            log_collector,
            toasts: egui_notify::Toasts::default(),
            tiles_tree,
            shared_plot_state: SharedPlotState::new(),
            active_view: View::System(0x01),
            sidebar_collapsed: true,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(feature = "profiling")]
        puffin::GlobalProfiler::lock().new_frame();

        #[cfg(feature = "profiling")]
        puffin::profile_function!();

        while let Ok(ack) = self.ack_rx.try_recv() {
            match ack.result {
                MavResult::Accepted => {
                    self.toasts
                        .success(format!("Command {:?} executed.", ack.command));
                }
                MavResult::InProgress => {}
                _ => {
                    self.toasts.error(format!(
                        "Command {:?} failed: {:?}.",
                        ack.command, ack.result
                    ));
                }
            }
        }

        // TODO
        if self.sidebar_collapsed {
            egui::SidePanel::left("sidepanel")
                .resizable(false)
                .exact_width(37.0)
                .show(ctx, |ui| {
                    ui.set_width(ui.available_width());

                    ui.selectable_value(&mut self.active_view, View::System(1), "🚁");
                    ui.selectable_value(&mut self.active_view, View::System(2), "✈");
                    ui.selectable_value(&mut self.active_view, View::System(3), "🚀");

                    ui.with_layout(egui::Layout::bottom_up(Align::LEFT), |ui| {
                        ui.add_space(5.0);
                        if ui.button("➡").clicked() {
                            self.sidebar_collapsed = false;
                        }
                        ui.separator();
                        ui.selectable_value(&mut self.active_view, View::Settings, "🔧");

                        #[cfg(feature = "profiling")]
                        {
                            let mut profiling_on = puffin::are_scopes_on();
                            ui.selectable_value(&mut profiling_on, true, "⏱");
                            puffin::set_scopes_on(profiling_on);
                        }
                    });
                });
        } else {
            egui::SidePanel::left("sidepanel")
                .resizable(true)
                .width_range(100.0..=1000.0)
                .default_width(800.0)
                .show(ctx, |ui| {
                    ui.set_width(ui.available_width());

                    for (i, system_id) in self.core.known_system_ids().iter().enumerate() {
                        if i != 0 {
                            ui.separator();
                        }

                        let Some(system) = self.core.system(*system_id) else {
                            continue;
                        };

                        ui.horizontal(|ui| {
                            ui.monospace(format!("0x{:02x}", system_id));
                            ui.heading(system.icon());
                            ui.label("Copter");
                            ui.monospace("AP v123");
                        });

                        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                            ui.selectable_value(
                                &mut self.active_view,
                                View::System(*system_id),
                                "Select ➡",
                            );
                        });
                    }

                    ui.with_layout(egui::Layout::bottom_up(Align::LEFT), |ui| {
                        ui.add_space(5.0);
                        if ui.button("⬅  Collapse").clicked() {
                            self.sidebar_collapsed = true;
                        }
                        ui.separator();
                        ui.selectable_value(&mut self.active_view, View::Settings, "🔧 Settings");
                        ui.selectable_value(&mut self.active_view, View::Overview, "🖧 Overview");
                    });
                });
        }

        egui::TopBottomPanel::bottom("bottombar")
            .resizable(true)
            .height_range(20.0..=1000.0)
            .default_height(200.0)
            .show(ctx, |ui| {
                ui.set_height(ui.available_height());
                ui.add_space(5.0);
                ui.add(egui_tracing::ui::Logs::new(self.log_collector.clone()));
            });

        ctx.input(|input| {
            if input.key_down(Key::Num0) {
                self.active_view = View::Overview;
            }

            let system_shortcuts = vec![
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
                Key::Num9,
            ];
            for (i, key) in system_shortcuts.iter().enumerate() {
                let sysid = (i + 1) as u8;
                if input.key_down(*key) && self.core.known_system_ids().contains(&sysid) {
                    self.active_view = View::System(sysid);
                }
            }
        });

        egui::CentralPanel::default()
            .frame(egui::Frame {
                inner_margin: Margin::ZERO,
                fill: ctx.style().visuals.window_fill(),
                ..Default::default()
            })
            .show(ctx, |ui| match self.active_view {
                View::Overview => {
                    ui.label("No system selected. TODO: put a map here as well.");
                }
                View::Settings => {
                    ui.label("TODO");
                }
                View::System(_) => {
                    let mut behavior = TreeBehavior {
                        shared_plot_state: &mut self.shared_plot_state,
                        core: self.core.clone(),
                        active_view: self.active_view,
                    };
                    self.tiles_tree.ui(&mut behavior, ui);
                }
            });

        self.toasts.show(ctx);

        #[cfg(feature = "profiling")]
        puffin_egui::show_viewport_if_enabled(ctx);
    }
}
