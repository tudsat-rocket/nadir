use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use egui::{Color32, Key, Margin, Modifiers};
use mavspec::rust::dialects::Common;
use mavspec::rust::dialects::common::enums::{MavCmd, MavResult};
use nadir_core::mav::{Event, V2};

#[allow(clippy::wildcard_imports)]
use crate::panes::*;
use crate::shell::{Sidebar, SidebarAction, StatusBar};
use crate::views::{LIVE, Overview, SettingsView, SourceId, View};
use crate::widgets::SharedPlotState;

pub struct App {
    #[cfg(not(target_arch = "wasm32"))]
    core: nadir_core::Core,
    live: nadir_core::Source,
    /// Telemetry logs opened alongside the live source. A map rather than a list because closing
    /// one must not renumber the others out from under a [`View`].
    logs: BTreeMap<SourceId, nadir_core::Source>,
    next_source_id: SourceId,
    event_rx: std::sync::mpsc::Receiver<Event<V2>>,
    /// Logs the file picker has read, by name and contents.
    #[cfg(target_arch = "wasm32")]
    picked_tx: std::sync::mpsc::Sender<(String, Vec<u8>)>,
    #[cfg(target_arch = "wasm32")]
    picked_rx: std::sync::mpsc::Receiver<(String, Vec<u8>)>,
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
        initial_logs: Vec<PathBuf>,
    ) -> Self {
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event<V2>>();
        #[cfg(target_arch = "wasm32")]
        let (picked_tx, picked_rx) = std::sync::mpsc::channel::<(String, Vec<u8>)>();
        let ctx2 = ctx.clone();

        // Only ever the defaults on wasm, where there is no file to read.
        let settings = nadir_core::Settings::load();
        SettingsView::apply_theme(ctx, settings.theme);

        #[cfg(not(target_arch = "wasm32"))]
        let core = {
            let mut builder = nadir_core::Core::builder();
            for link in &settings.links {
                builder = builder.link(link.clone());
            }
            if settings.autoconnect_usb {
                builder = builder.autoconnect_to_usb();
            }

            builder
                .on_event(Box::new(move |event| {
                    let _ = event_tx.send(event.clone());
                    ctx2.request_repaint_after(std::time::Duration::from_millis(16));
                }))
                .spawn()
        };

        #[cfg(not(target_arch = "wasm32"))]
        let live = core.live.clone();

        #[cfg(target_arch = "wasm32")]
        let live = {
            // Nothing feeds this yet: no links here, and no stream transport written.
            let _ = (&event_tx, &ctx2);
            nadir_core::Source::detached()
        };

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

        // A phone fits one pane at a time, not a grid of four tab bars.
        let root = if cfg!(target_os = "android") {
            let tabs: Vec<_> = [
                map, preflight, propulsion, state, sensors, navigation, mission, messages,
                commands, params, plot, can, flight_log,
            ]
            .into_iter()
            .chain(profiler)
            .collect();

            tiles.insert_tab_tile(tabs)
        } else {
            let top_left = vec![propulsion, params];
            let top_right = vec![state, preflight, navigation, mission];
            let bottom_left = vec![map, messages, commands, flight_log];
            let bottom_right: Vec<_> = [sensors, plot, can].into_iter().chain(profiler).collect();

            let cells = [top_left, top_right, bottom_left, bottom_right]
                .map(|group| tiles.insert_tab_tile(group));
            tiles.insert_grid_tile(cells.to_vec())
        };

        let tiles_tree = egui_tiles::Tree::new("my_tree", root, tiles);

        #[allow(unused_mut)]
        let mut app = Self {
            #[cfg(not(target_arch = "wasm32"))]
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
            #[cfg(target_arch = "wasm32")]
            picked_tx,
            #[cfg(target_arch = "wasm32")]
            picked_rx,
            sidebar: Sidebar::new(),
            status_bar: StatusBar::new(ctx),
            overview: Overview::new(),
            settings: SettingsView::new(&settings),
            shared_plot_state: SharedPlotState::new(),
            position_source: PositionSource::default(),
            active_view: View::Overview,
            never_connected: true,
            logs_shown: false,
        };

        // A browser has no path to open; a log dropped there arrives as bytes instead.
        #[cfg(target_arch = "wasm32")]
        std::mem::drop(initial_logs);
        #[cfg(not(target_arch = "wasm32"))]
        for path in initial_logs {
            app.open_log(&path);
        }

        app
    }

    /// The source a view names, or `None` once a log it pointed at has been closed.
    fn source(&self, id: SourceId) -> Option<&nadir_core::Source> {
        if id == LIVE {
            Some(&self.live)
        } else {
            self.logs.get(&id)
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_log(&mut self, path: &Path) {
        self.add_log(path, nadir_core::Source::open_log(path));
    }

    /// A log the browser handed over whole, since there is no path there to open.
    #[cfg(target_arch = "wasm32")]
    fn open_log_bytes(&mut self, name: &str, bytes: impl AsRef<[u8]> + 'static) {
        let opened = nadir_core::Source::open_log_bytes(name, bytes);
        self.add_log(Path::new(name), opened);
    }

    fn add_log(&mut self, path: &Path, opened: Result<nadir_core::Source, nadir_core::LogError>) {
        let already_open = self.logs.values().any(|source| match &source.origin {
            nadir_core::Origin::Log(progress) => progress.path == path,
            nadir_core::Origin::Live => false,
        });

        // Only reachable by dropping a file that is already listed as open in the overview.
        if already_open {
            self.toasts
                .info(format!("{} is already open.", path.display()));
            return;
        }

        match opened {
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

    /// Asks for a telemetry log to open. Blocks on the native dialog, as the log downloader's save
    /// dialog does.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    fn pick_log(&mut self, _ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Telemetry log", &["tlog"])
            .pick_file()
        {
            self.open_log(&path);
        }
    }

    /// Android has no file picker: `rfd` has no backend, and shared storage would mean the Storage
    /// Access Framework over JNI. Logs recorded on the device are still listed.
    #[cfg(target_os = "android")]
    fn pick_log(&mut self, _ctx: &egui::Context) {
        self.toasts
            .warning("Opening a log file is not supported on Android.");
    }

    /// The same, for a browser: the picker is a promise, and the file arrives as bytes on the
    /// channel `update` drains.
    #[cfg(target_arch = "wasm32")]
    fn pick_log(&mut self, ctx: &egui::Context) {
        let picked_tx = self.picked_tx.clone();
        let ctx = ctx.clone();

        wasm_bindgen_futures::spawn_local(async move {
            let Some(handle) = rfd::AsyncFileDialog::new()
                .add_filter("Telemetry log", &["tlog"])
                .pick_file()
                .await
            else {
                return;
            };

            let _ = picked_tx.send((handle.file_name(), handle.read().await));
            ctx.request_repaint();
        });
    }

    fn close_log(&mut self, id: SourceId) {
        if let Some(source) = self.logs.remove(&id) {
            // The loader holds its own handle, so dropping ours does not stop it on its own.
            if let nadir_core::Origin::Log(progress) = &source.origin {
                progress.cancel();
            }
        }

        if self.active_view.source() == Some(id) {
            self.active_view = View::Overview;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

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

        #[cfg(target_arch = "wasm32")]
        while let Ok((name, bytes)) = self.picked_rx.try_recv() {
            self.open_log_bytes(&name, bytes);
        }

        #[cfg(not(target_arch = "wasm32"))]
        for path in dropped_logs(&ctx) {
            self.open_log(&path);
        }

        // Same channel as the file picker, since the read can only be awaited off-frame.
        #[cfg(target_arch = "wasm32")]
        for file in dropped_logs(&ctx) {
            let picked_tx = self.picked_tx.clone();
            let ctx = ctx.clone();

            wasm_bindgen_futures::spawn_local(async move {
                let name = file.path().to_string_lossy().into_owned();
                if let Ok(bytes) = file.bytes_async().await {
                    let _ = picked_tx.send((name, bytes));
                    ctx.request_repaint();
                }
            });
        }

        if self.logs.values().any(|source| match &source.origin {
            nadir_core::Origin::Log(progress) => !progress.done(),
            nadir_core::Origin::Live => false,
        }) {
            ctx.request_repaint();
        }

        let action = self.sidebar.show(
            ui,
            &self.live,
            &self.logs,
            &mut self.active_view,
            &mut self.logs_shown,
        );
        match action {
            Some(SidebarAction::OpenLog) => self.pick_log(&ctx),
            Some(SidebarAction::CloseLog(id)) => self.close_log(id),
            None => {}
        }

        if self.logs_shown {
            egui::Panel::bottom("bottombar")
                .resizable(true)
                .size_range(20.0..=1000.0)
                .default_size(200.0)
                .show(ui, |ui| {
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
            self.status_bar.show(ui, &system, &mut behavior);
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
                fill: ctx.global_style().visuals.window_fill(),
                ..Default::default()
            })
            .show(ui, |ui| match self.active_view {
                View::Overview => {
                    #[cfg(not(target_arch = "wasm32"))]
                    let links = self.core.links();
                    #[cfg(target_arch = "wasm32")]
                    let links = Vec::new();

                    to_open = self.overview.ui(ui, &links, &self.logs);
                }
                View::Settings => {
                    self.settings.ui(ui);
                }
                View::System { .. } => {
                    self.tiles_tree.ui(&mut behavior, ui);
                }
            });

        // Only ever `Some` where there is a log directory to list, which a browser has not.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = to_open {
            self.open_log(&path);
        }

        // egui-notify paints toast backgrounds with the global noninteractive bg_fill; push it away
        // from the panel just for the toast pass (nothing else paints after this) so notifications
        // stand out. Which way is away depends on the theme: extreme_bg_color is the darkest color
        // under one and the lightest under the other.
        let original_bg = ctx.global_style().visuals.widgets.noninteractive.bg_fill;
        ctx.global_style_mut(|s| {
            s.visuals.widgets.noninteractive.bg_fill = if s.visuals.dark_mode {
                s.visuals.extreme_bg_color
            } else {
                s.visuals.widgets.inactive.weak_bg_fill
            };
        });
        self.toasts.show(&ctx);
        ctx.global_style_mut(|s| s.visuals.widgets.noninteractive.bg_fill = original_bg);
    }
}

/// Paths dropped onto the window that look like telemetry logs.
#[cfg(not(target_arch = "wasm32"))]
fn dropped_logs(ctx: &egui::Context) -> Vec<PathBuf> {
    ctx.input(|input| {
        input
            .raw
            .dropped_files
            .iter()
            .map(|file| file.path().to_path_buf())
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("tlog"))
            })
            .collect()
    })
}

/// The same, in a browser, where `path` is only the file name and the contents have to be read
/// asynchronously, so the handles are returned and read by the caller.
#[cfg(target_arch = "wasm32")]
fn dropped_logs(ctx: &egui::Context) -> Vec<std::sync::Arc<dyn egui::DroppedFile + Send + Sync>> {
    ctx.input(|input| {
        input
            .raw
            .dropped_files
            .iter()
            .filter(|file| {
                file.path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("tlog"))
            })
            .cloned()
            .collect()
    })
}
