use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use egui::{Color32, Key, Margin, Modifiers};
use maviola::asnc::node::Event;
use maviola::prelude::V2;
use mavspec::rust::dialects::Common;
use mavspec::rust::dialects::common::enums::{MavCmd, MavResult};

#[allow(clippy::wildcard_imports)]
use crate::panes::*;
use crate::shell::{Sidebar, StatusBar};
use crate::views::{LIVE, Overview, SettingsView, SourceId, View};
use crate::widgets::SharedPlotState;

pub struct App {
    core: core::Core,
    live: core::Source,
    /// Telemetry logs opened alongside the live source. A map rather than a list because closing
    /// one must not renumber the others out from under a [`View`].
    logs: BTreeMap<SourceId, core::Source>,
    next_source_id: SourceId,
    event_rx: std::sync::mpsc::Receiver<Event<V2>>,
    log_collector: egui_tracing::tracing::collector::EventCollector,
    toasts: egui_notify::Toasts,
    tiles_tree: egui_tiles::Tree<Pane>,
    sidebar: Sidebar,
    status_bar: StatusBar,
    overview: Overview,
    settings: SettingsView,
    shared_plot_state: SharedPlotState,
    position_source: PositionSource,
    active_view: View,
    never_connected: bool,
    logs_shown: bool,
}

impl App {
    pub fn new(
        log_collector: egui_tracing::tracing::collector::EventCollector,
        ctx: &egui::Context,
    ) -> Self {
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event<V2>>();
        let ctx2 = ctx.clone();

        let settings = core::Settings::load();

        let mut builder = core::Core::builder();
        for link in &settings.links {
            builder = builder.link(link.clone());
        }
        if settings.autoconnect_usb {
            builder = builder.autoconnect_to_usb();
        }

        let core = builder
            .on_event(Box::new(move |event| {
                let _ = event_tx.send(event.clone());
                ctx2.request_repaint_after(std::time::Duration::from_millis(16));
            }))
            .spawn();

        let live = core.live.clone();

        let mut tiles = egui_tiles::Tiles::default();

        let map = tiles.insert_pane(Pane::Map(Box::new(MapPane::new(
            ctx,
            settings.map.mapbox_access_token.clone(),
        ))));
        let propulsion = tiles.insert_pane(Pane::Propulsion(PropulsionPane::new(ctx)));
        let preflight = tiles.insert_pane(Pane::Preflight(PreflightPane::new(ctx)));
        let navigation = tiles.insert_pane(Pane::Navigation(NavigationPane::new(ctx)));
        let mission = tiles.insert_pane(Pane::Placeholder("Mission".to_owned()));
        let state = tiles.insert_pane(Pane::StateEstimator(StateEstimatorPane::new(ctx)));
        let sensors = tiles.insert_pane(Pane::Sensors(SensorsPane::new(ctx)));
        let plot = tiles.insert_pane(Pane::Plot(PlotPane::new(ctx)));
        let messages = tiles.insert_pane(Pane::Messages(MessagesPane::new(ctx)));
        let commands = tiles.insert_pane(Pane::Commands(CommandsPane::new(ctx)));
        let params = tiles.insert_pane(Pane::Params(ParamsPane::new(ctx)));
        let can = tiles.insert_pane(Pane::CanProbe(CanProbePane::new(ctx)));
        let flight_log = tiles.insert_pane(Pane::FlightLogs(LogsPane::new(ctx)));

        #[cfg(feature = "profiling")]
        let profiler = Some(tiles.insert_pane(Pane::Profiler));
        #[cfg(not(feature = "profiling"))]
        let profiler: Option<egui_tiles::TileId> = None;

        let top_left_tabs = tiles.insert_tab_tile(vec![propulsion, params]);
        let bottom_left_tabs = tiles.insert_tab_tile(vec![map, messages, commands, flight_log]);

        let top_right_tabs = tiles.insert_tab_tile(vec![state, preflight, navigation, mission]);
        let bottom_right_tabs =
            tiles.insert_tab_tile([sensors, plot, can].into_iter().chain(profiler).collect());

        let bottom = tiles.insert_grid_tile(vec![
            top_left_tabs,
            top_right_tabs,
            bottom_left_tabs,
            bottom_right_tabs,
        ]);

        let tiles_tree = egui_tiles::Tree::new("my_tree", bottom, tiles);

        Self {
            core,
            live,
            event_rx,
            log_collector,
            toasts: egui_notify::Toasts::default()
                .with_anchor(egui_notify::Anchor::TopRight)
                .with_shadow(egui::epaint::Shadow {
                    offset: [0, 3],
                    blur: 10,
                    spread: 0,
                    color: Color32::from_black_alpha(160),
                }),
            tiles_tree,
            logs: BTreeMap::new(),
            next_source_id: LIVE + 1,
            sidebar: Sidebar::new(),
            status_bar: StatusBar::new(ctx),
            overview: Overview::new(),
            settings: SettingsView::new(&settings),
            shared_plot_state: SharedPlotState::new(),
            position_source: PositionSource::default(),
            active_view: View::Overview,
            never_connected: true,
            logs_shown: false,
        }
    }

    /// The source a view names, or `None` once a log it pointed at has been closed.
    fn source(&self, id: SourceId) -> Option<&core::Source> {
        if id == LIVE {
            Some(&self.live)
        } else {
            self.logs.get(&id)
        }
    }

    fn open_log(&mut self, path: &Path) {
        let already_open = self.logs.values().any(|source| match &source.origin {
            core::Origin::Log(progress) => progress.path == path,
            core::Origin::Live => false,
        });

        // Only reachable by dropping a file that is already listed as open in the overview.
        if already_open {
            self.toasts
                .info(format!("{} is already open.", path.display()));
            return;
        }

        match core::Source::open_log(path) {
            Ok(source) => {
                let id = self.next_source_id;
                self.next_source_id += 1;
                self.logs.insert(id, source);
                self.overview.refresh();

                self.toasts
                    .success(format!("Opened {}.", path.display()))
                    .duration(Some(std::time::Duration::from_secs(4)));
            }
            Err(e) => {
                tracing::error!("Failed to open {}: {e}", path.display());
                self.toasts.error(format!("{e}"));
            }
        }
    }

    fn close_log(&mut self, id: SourceId) {
        if let Some(source) = self.logs.remove(&id) {
            // The loader holds its own handle, so dropping ours does not stop it on its own.
            if let core::Origin::Log(progress) = &source.origin {
                progress.cancel();
            }
        }

        if self.active_view.source() == Some(id) {
            self.active_view = View::Overview;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(feature = "profiling")]
        puffin::GlobalProfiler::lock().new_frame();

        #[cfg(feature = "profiling")]
        puffin::profile_function!();

        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                Event::Frame(frame, _callback) => {
                    if let Ok(Common::CommandAck(ack)) = frame.decode() {
                        if ack.command == MavCmd::RequestMessage
                            || ack.command == MavCmd::SetMessageInterval
                        {
                            continue;
                        }

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
                }
                Event::NewPeer(peer) => {
                    if self.never_connected && self.active_view == View::Overview {
                        self.active_view = View::system(LIVE, peer.system_id());
                        self.never_connected = false;
                        self.sidebar.collapse();
                    }

                    self.toasts
                        .success(format!("System 0x{:02x} connected.", peer.system_id()));
                }
                Event::PeerLost(peer) => {
                    self.toasts
                        .warning(format!("System 0x{:02x} lost.", peer.system_id()));
                }
                Event::Invalid(..) => {}
            }
        }

        for path in dropped_logs(ctx) {
            self.open_log(&path);
        }

        let close = self.sidebar.show(
            ctx,
            &self.live,
            &self.logs,
            &mut self.active_view,
            &mut self.logs_shown,
        );
        if let Some(id) = close {
            self.close_log(id);
        }

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

        let active_source = match self.active_view {
            View::System { source, .. } => {
                let source = self.source(source).cloned();

                // The log this view pointed at has been closed.
                if source.is_none() {
                    self.active_view = View::Overview;
                }

                source
            }
            View::Overview | View::Settings => None,
        };

        let mut behavior = TreeBehavior {
            shared_plot_state: &mut self.shared_plot_state,
            // The fallback is never read; nothing outside `View::System` draws through this.
            source: active_source.clone().unwrap_or_else(|| self.live.clone()),
            active_view: self.active_view,
            position_source: &mut self.position_source,
        };

        if let View::System { system_id, .. } = self.active_view
            && let Some(system) = active_source.and_then(|source| source.system(system_id))
        {
            self.status_bar.show(ctx, &system, &mut behavior);
        }

        ctx.input(|input| {
            if input.key_down(Key::Num0)
                && input.modifiers.contains(Modifiers::CTRL)
                && input.modifiers.contains(Modifiers::SHIFT)
            {
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
                if input.key_down(*key)
                    && input.modifiers.contains(Modifiers::CTRL)
                    && input.modifiers.contains(Modifiers::SHIFT)
                    && self.live.known_system_ids().contains(&sysid)
                {
                    self.active_view = View::system(LIVE, sysid);
                }
            }
        });

        let mut to_open = None;

        egui::CentralPanel::default()
            .frame(egui::Frame {
                inner_margin: Margin::ZERO,
                fill: ctx.style().visuals.window_fill(),
                ..Default::default()
            })
            .show(ctx, |ui| match self.active_view {
                View::Overview => {
                    let links = self.core.links();
                    to_open = self.overview.ui(ui, &links, &self.logs);
                }
                View::Settings => {
                    self.settings.ui(ui);
                }
                View::System { .. } => {
                    self.tiles_tree.ui(&mut behavior, ui);
                }
            });

        if let Some(path) = to_open {
            self.open_log(&path);
        }

        // egui-notify paints toast backgrounds with the global noninteractive bg_fill; darken it
        // just for the toast pass (nothing else paints after this) so notifications stand out.
        let original_bg = ctx.style().visuals.widgets.noninteractive.bg_fill;
        ctx.style_mut(|s| s.visuals.widgets.noninteractive.bg_fill = s.visuals.extreme_bg_color);
        self.toasts.show(ctx);
        ctx.style_mut(|s| s.visuals.widgets.noninteractive.bg_fill = original_bg);
    }
}

/// Paths dropped onto the window that look like telemetry logs.
fn dropped_logs(ctx: &egui::Context) -> Vec<PathBuf> {
    ctx.input(|input| {
        input
            .raw
            .dropped_files
            .iter()
            .filter_map(|file| file.path.clone())
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("tlog"))
            })
            .collect()
    })
}
