use core::LinkId;

use egui::{Align, Color32, Key, Layout, Margin, RichText};
use egui_tiles::LinearDir;
use mavspec::rust::dialects::common::enums::MavResult;
use mavspec::rust::dialects::common::messages::CommandAck;

use crate::panes::*;
use crate::views::View;
use crate::widgets::{
    ArmedIndicator, AutopilotLogo, MavStateIndicator, ModeDisplay, SharedPlotState,
};

pub struct App {
    core: core::Core,
    ack_rx: std::sync::mpsc::Receiver<CommandAck>,
    log_collector: egui_tracing::tracing::collector::EventCollector,
    toasts: egui_notify::Toasts,
    tiles_tree: egui_tiles::Tree<Pane>,
    shared_plot_state: SharedPlotState,
    active_view: View,
    sidebar_collapsed: bool,
    logs_shown: bool,
}

impl App {
    pub fn new(
        log_collector: egui_tracing::tracing::collector::EventCollector,
        ctx: &egui::Context,
    ) -> Self {
        let (ack_tx, ack_rx) = std::sync::mpsc::channel::<CommandAck>();
        let ctx2 = ctx.clone();

        let core = core::Core::builder()
            .udp_server("0.0.0.0:14550".parse().unwrap())
            .tcp_client("127.0.0.1:5760".parse().unwrap())
            .tcp_client("127.0.0.1:5761".parse().unwrap())
            .tcp_client("127.0.0.1:5762".parse().unwrap())
            .autoconnect_to_usb()
            .on_ack(Box::new(move |ack| {
                ack_tx.send(ack.clone()).unwrap();
            }))
            .on_event(Box::new(move |_event| {
                ctx2.request_repaint();
            }))
            .spawn();

        let mut tiles = egui_tiles::Tiles::default();

        let map = tiles.insert_pane(Pane::Map(MapPane::new(ctx, None)));

        let status = tiles.insert_pane(Pane::Status(StatusPane::new(ctx)));
        let components = tiles.insert_pane(Pane::Placeholder("Info".to_owned()));
        let link = tiles.insert_pane(Pane::Links(LinksPane::new(ctx)));
        let top_tabs = tiles.insert_tab_tile(vec![status, components, link]);

        let system = tiles.insert_pane(Pane::Placeholder("System Overview".to_owned()));
        let state = tiles.insert_pane(Pane::StateEstimator(StateEstimatorPane::new(ctx)));
        let sensors = tiles.insert_pane(Pane::Sensors(SensorsPane::new(ctx)));
        let plot = tiles.insert_pane(Pane::Plot(PlotPane::new(ctx)));
        let messages = tiles.insert_pane(Pane::Messages(MessagesPane::new(ctx)));
        let commands = tiles.insert_pane(Pane::Commands(CommandsPane::new(ctx)));
        let can = tiles.insert_pane(Pane::CanProbe(CanProbePane::new(ctx)));
        let bottom_tabs =
            tiles.insert_tab_tile(vec![system, state, sensors, plot, messages, commands, can]);

        let side = tiles.insert_new(egui_tiles::Tile::Container(egui_tiles::Container::Linear(
            egui_tiles::Linear::new_binary(LinearDir::Vertical, [top_tabs, bottom_tabs], 0.35),
        )));

        let root = tiles.insert_new(egui_tiles::Tile::Container(egui_tiles::Container::Linear(
            egui_tiles::Linear::new_binary(LinearDir::Horizontal, [side, map], 0.4),
        )));

        let tiles_tree = egui_tiles::Tree::new("my_tree", root, tiles);

        Self {
            core,
            ack_rx,
            log_collector,
            toasts: egui_notify::Toasts::default().with_anchor(egui_notify::Anchor::BottomRight),
            tiles_tree,
            shared_plot_state: SharedPlotState::new(),
            active_view: View::Overview,
            sidebar_collapsed: false,
            logs_shown: false,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(feature = "profiling")]
        puffin::GlobalProfiler::lock().new_frame();

        #[cfg(feature = "profiling")]
        puffin::profile_function!();

        ctx.set_zoom_factor(1.25);

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

        let clps = self.sidebar_collapsed;
        egui::SidePanel::left("sidepanel")
            .resizable(false)
            .exact_width(if clps { 37.0 } else { 300.0 })
            .show(ctx, |ui| {
                ui.set_width(ui.available_width());

                for (i, system_id) in self.core.known_system_ids().iter().enumerate() {
                    if i != 0 && !self.sidebar_collapsed {
                        ui.separator();
                    }

                    let Some(system) = self.core.system(*system_id) else {
                        continue;
                    };

                    if self.sidebar_collapsed {
                        ui.selectable_value(
                            &mut self.active_view,
                            View::System(*system_id),
                            system.icon(),
                        );
                    } else if let Some(heartbeat) = system.last_heartbeat().ok().flatten() {
                        ui.horizontal(|ui| {
                            ui.monospace(format!("0x{:02x}", system_id));
                            ui.label(system.icon());

                            ui.add(ArmedIndicator(heartbeat.base_mode));

                            ui.place(ui.available_rect_before_wrap(), |ui: &mut egui::Ui| {
                                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                                    ui.add_space(5.0);
                                    ui.add(AutopilotLogo(heartbeat.autopilot, heartbeat.type_));
                                })
                                .response
                            });
                        });

                        ui.horizontal(|ui| {
                            ui.add(ModeDisplay::new(system.clone()));
                        });

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("🔋 98%").color(Color32::from_rgb(78, 154, 6)));
                            ui.place(ui.available_rect_before_wrap(), |ui: &mut egui::Ui| {
                                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                                    ui.selectable_value(
                                        &mut self.active_view,
                                        View::System(*system_id),
                                        "Select ➡",
                                    );

                                    let total_data_rate = system
                                        .channels()
                                        .iter_mut()
                                        .map(|(_, s)| s.received_data_rate())
                                        .sum::<f32>()
                                        / 1024.0;
                                    ui.label("KiB/s");
                                    ui.monospace(format!("{:>5.2}", total_data_rate));
                                    ui.weak("⏬");
                                })
                                .response
                            });
                        });
                    } else {
                        ui.monospace(format!("0x{:02x}", system_id));
                        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                            ui.selectable_value(
                                &mut self.active_view,
                                View::System(*system_id),
                                "Select ➡",
                            );
                        });
                    }
                }

                ui.with_layout(egui::Layout::bottom_up(Align::LEFT), |ui| {
                    ui.add_space(5.0);
                    if ui.button(if clps { "➡" } else { "⬅  Collapse" }).clicked() {
                        self.sidebar_collapsed = !self.sidebar_collapsed;
                    }
                    ui.separator();

                    #[cfg(feature = "profiling")]
                    {
                        let mut profiling_on = puffin::are_scopes_on();
                        ui.selectable_value(
                            &mut profiling_on,
                            true,
                            if clps { "⏱" } else { "⏱ Profiling" },
                        );
                        puffin::set_scopes_on(profiling_on);
                    }

                    ui.toggle_value(
                        &mut self.logs_shown,
                        if clps { "📃" } else { "📃 Show Debug Logs" },
                    );

                    ui.separator();

                    ui.selectable_value(
                        &mut self.active_view,
                        View::Settings,
                        if clps { "🔧" } else { "🔧 Preferences" },
                    );

                    ui.selectable_value(
                        &mut self.active_view,
                        View::Overview,
                        if clps { "🖧" } else { "🖧 Overview" },
                    );
                });
            });

        if self.logs_shown {
            egui::TopBottomPanel::bottom("bottombar")
                .resizable(true)
                .height_range(20.0..=1000.0)
                .default_height(200.0)
                .show(ctx, |ui| {
                    ui.set_height(ui.available_height());
                    ui.add_space(5.0);
                    ui.add(egui_tracing::ui::Logs::new(self.log_collector.clone()));
                });
        }

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

                    for link in self.core.links() {
                        let mut stats = link.stats;
                        let info_string = match link.id {
                            LinkId::UdpServer(addr) => format!("udp:{addr}"),
                            LinkId::TcpClient(addr) => format!("tcp:{addr}"),
                            LinkId::SerialPort(port) => format!("serial:{port}"),
                        };

                        ui.horizontal(|ui| {
                            ui.add_space(5.0);
                            ui.weak("🖧");
                            ui.label(info_string);
                        });

                        ui.horizontal(|ui| {
                            ui.add_space(5.0);
                            ui.weak("⏬");
                            ui.monospace(format!("{:>3.0}", stats.received_packet_rate()));
                            ui.label("pkt/s ");
                            ui.monospace(format!("{:>5.2}", stats.received_data_rate() / 1024.0));
                            ui.label("KiB/s");
                        });
                    }
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
