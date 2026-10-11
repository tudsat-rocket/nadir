//! Contains our map widget, based on the walkers crate.

#[cfg(not(target_arch = "wasm32"))]
pub mod tiles;

use nadir_core::System;
use nadir_core::settings::{MapSettings, MapboxStyle};
use std::collections::{BTreeSet, HashMap, VecDeque};

use chrono::{DateTime, Utc};

use eframe::egui;
use egui::{
    Align, Color32, Frame, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, UiBuilder, Vec2,
};
use mavspec::rust::dialects::common::messages::{
    GlobalPositionInt, Heartbeat, HomePosition, PositionTargetGlobalInt,
};
use walkers::sources::Mapbox;
use walkers::{MapMemory, Plugin, Position, Projector, Tiles};
use walkers_extras::{LabeledSymbol, LabeledSymbolStyle, Places, Symbol};

use crate::{
    colors::mode_color,
    panes::{PaneUi, TreeBehavior},
    theme::OVERLAY_SHADOW,
    views::View,
};

/// The zoom the map shows a single point at, and the closest it zooms in to fit several.
const FIT_MAX_ZOOM: f64 = 17.0;
const FIT_MIN_ZOOM: f64 = 3.0;
/// Room around the fitted points, so markers and their labels clear the overlays.
const FIT_PADDING: f32 = 80.0;

#[derive(Clone, Copy)]
struct PathPoint {
    altitude: f64,
    custom_mode: u32,
}

#[derive(Default)]
struct SystemPath {
    points: Vec<(Position, PathPoint)>,
    /// Number of `GLOBAL_POSITION_INT`s read so far, which stops matching `points` once cleared.
    ingested: usize,
    last_gps: Option<DateTime<Utc>>,
    last_heartbeat: Option<DateTime<Utc>>,
    /// Heartbeats newer than the last appended GPS point, waiting to be joined once position
    /// messages catch up.
    pending_heartbeats: VecDeque<(DateTime<Utc>, u32)>,
    custom_mode: u32,
    altitude_range: Option<(f64, f64)>,
    modes: BTreeSet<u32>,
}

impl SystemPath {
    fn clear(&mut self) {
        self.points.clear();
        self.altitude_range = None;
        self.modes.clear();
    }
}

struct MapboxLayer {
    access_token: String,
    style: MapboxStyle,
    tiles: Box<dyn Tiles>,
}

pub struct MapPane {
    ctx: egui::Context,
    osm_tiles: Box<dyn Tiles>,
    mapbox: Option<MapboxLayer>,
    show_mapbox: bool,
    memory: MapMemory,
    /// Keeps home and every vehicle in view, until the user pans or zooms.
    fit: bool,
    center: Option<Position>,
    visualization: Visualization,
    gradient: Vec<Color32>,
    system_paths: HashMap<u8, SystemPath>,
}

struct NavigationPlugin {
    system: nadir_core::System,
}

impl walkers::Plugin for NavigationPlugin {
    fn run(
        self: Box<Self>,
        _ui: &mut Ui,
        response: &egui::Response,
        projector: &Projector,
        _map_memory: &MapMemory,
    ) {
        if let Some(screen_pos) = response.interact_pointer_pos()
            && response.secondary_clicked()
            && !self.system.muted()
        {
            let world_pos = projector.unproject(egui::Vec2::new(screen_pos.x, screen_pos.y));

            // TODO: get the altitude from somewhere, make reference frame selectable.
            self.system
                .do_reposition(world_pos.y(), world_pos.x(), 50.0);
        }
    }
}

struct LinePlugin {
    a: Position,
    b: Position,
    color: Color32,
}

impl walkers::Plugin for LinePlugin {
    fn run(
        self: Box<Self>,
        ui: &mut Ui,
        _response: &egui::Response,
        projector: &Projector,
        _map_memory: &MapMemory,
    ) {
        let a_pos = projector.project(self.a);
        let b_pos = projector.project(self.b);

        let shape = Shape::dashed_line(
            &[Pos2::new(a_pos.x, a_pos.y), Pos2::new(b_pos.x, b_pos.y)],
            Stroke::new(2.0_f32, self.color),
            12.0,
            8.0,
        );

        ui.painter().add(shape);
    }
}

/// A wedge outside each vehicle's marker, pointing where it is headed.
struct HeadingPlugin {
    headings: Vec<(Position, f32)>,
    radius: f32,
    color: Color32,
}

impl walkers::Plugin for HeadingPlugin {
    fn run(
        self: Box<Self>,
        ui: &mut Ui,
        _response: &egui::Response,
        projector: &Projector,
        _map_memory: &MapMemory,
    ) {
        for (position, heading) in self.headings {
            let center = projector.project(position).to_pos2();
            let (sin, cos) = heading.sin_cos();
            let forward = Vec2::new(sin, -cos);
            let side = Vec2::new(cos, sin);

            ui.painter().add(Shape::convex_polygon(
                vec![
                    center + forward * (self.radius + 9.0),
                    center + forward * (self.radius - 2.0) + side * 6.0,
                    center + forward * (self.radius - 2.0) - side * 6.0,
                ],
                self.color,
                Stroke::NONE,
            ));
        }
    }
}

impl MapPane {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            ctx: ctx.clone(),
            osm_tiles: Self::osm_tiles(ctx),
            mapbox: None,
            show_mapbox: false,
            memory: MapMemory::default(),
            fit: true,
            center: None,
            visualization: Visualization::Altitude,
            gradient: build_gradient(1001),
            system_paths: HashMap::new(),
        }
    }

    /// Where the map is looking, once it has been drawn.
    pub fn center(&self) -> Option<Position> {
        self.center
    }

    pub fn style_label(style: MapboxStyle) -> &'static str {
        match style {
            MapboxStyle::Satellite => "Satellite",
            MapboxStyle::SatelliteStreets => "Satellite Streets",
            MapboxStyle::Streets => "Streets",
            MapboxStyle::Outdoors => "Outdoors",
            MapboxStyle::Light => "Light",
            MapboxStyle::Dark => "Dark",
        }
    }

    /// The tile source for a style, and a name for it that stays unique across styles.
    pub fn mapbox_source(style: MapboxStyle, access_token: String) -> (Mapbox, &'static str) {
        use walkers::sources::MapboxStyle as Walkers;

        let (style, slug) = match style {
            MapboxStyle::Satellite => (Walkers::Satellite, "satellite-v9"),
            MapboxStyle::SatelliteStreets => (Walkers::SatelliteStreets, "satellite-streets-v12"),
            MapboxStyle::Streets => (Walkers::Streets, "streets-v12"),
            MapboxStyle::Outdoors => (Walkers::Outdoors, "outdoors-v12"),
            MapboxStyle::Light => (Walkers::Light, "light-v11"),
            MapboxStyle::Dark => (Walkers::Dark, "dark-v11"),
        };

        let source = Mapbox {
            style,
            access_token,
            high_resolution: true,
        };

        (source, slug)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn osm_tiles(ctx: &egui::Context) -> Box<dyn Tiles> {
        Box::new(tiles::CachedTiles::new(&tiles::Store::osm(), ctx))
    }

    /// The browser caches tiles itself.
    #[cfg(target_arch = "wasm32")]
    fn osm_tiles(ctx: &egui::Context) -> Box<dyn Tiles> {
        Box::new(walkers::HttpTiles::new(
            walkers::sources::OpenStreetMap,
            ctx.clone(),
        ))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn mapbox_tiles(
        ctx: &egui::Context,
        style: MapboxStyle,
        access_token: String,
    ) -> Box<dyn Tiles> {
        Box::new(tiles::CachedTiles::new(
            &tiles::Store::mapbox(style, access_token),
            ctx,
        ))
    }

    #[cfg(target_arch = "wasm32")]
    fn mapbox_tiles(
        ctx: &egui::Context,
        style: MapboxStyle,
        access_token: String,
    ) -> Box<dyn Tiles> {
        let (source, _) = Self::mapbox_source(style, access_token);
        Box::new(walkers::HttpTiles::new(source, ctx.clone()))
    }

    /// Rebuilds the Mapbox layer when its token or style changed in the settings.
    fn sync_mapbox(&mut self, settings: &MapSettings) {
        let access_token = settings
            .mapbox_access_token
            .clone()
            .or(option_env!("MAPBOX_ACCESS_TOKEN").map(str::to_owned));

        let Some(access_token) = access_token else {
            self.mapbox = None;
            return;
        };

        if self.mapbox.as_ref().is_some_and(|layer| {
            layer.access_token == access_token && layer.style == settings.mapbox_style
        }) {
            return;
        }

        // Mapbox is the default whenever there is one.
        self.show_mapbox |= self.mapbox.is_none();
        self.mapbox = Some(MapboxLayer {
            tiles: Self::mapbox_tiles(&self.ctx, settings.mapbox_style, access_token.clone()),
            access_token,
            style: settings.mapbox_style,
        });
    }

    /// Appends newly arrived positions to each system's cached path.
    fn ingest(&mut self, behavior: &TreeBehavior, system_ids: &[u8]) {
        for s_id in system_ids {
            let Some(system) = behavior.source.system(*s_id) else {
                continue;
            };
            let count = system.message_count::<GlobalPositionInt>();
            let path = self.system_paths.entry(*s_id).or_default();
            if count == path.ingested {
                continue;
            }
            path.ingested = count;

            let new_gps = system.messages_since::<GlobalPositionInt>(path.last_gps, None);
            let new_heartbeats = system.messages_since::<Heartbeat>(path.last_heartbeat, None);

            for (ts, hb) in new_heartbeats {
                path.pending_heartbeats.push_back((ts, hb.custom_mode));
                path.last_heartbeat = Some(ts);
            }

            for (ts, gps) in new_gps {
                path.last_gps = Some(ts);

                // Advance the flight mode to the last heartbeat at or before this GPS timestamp;
                // later ones stay pending for future points.
                while let Some((hb_ts, mode)) = path.pending_heartbeats.front().copied() {
                    if hb_ts > ts {
                        break;
                    }
                    path.custom_mode = mode;
                    path.pending_heartbeats.pop_front();
                }

                let Some(position) = Self::position(gps.lat, gps.lon) else {
                    continue;
                };
                let altitude = f64::from(gps.relative_alt) / 1000.0;
                path.altitude_range = Some(match path.altitude_range {
                    Some((min, max)) => (min.min(altitude), max.max(altitude)),
                    None => (altitude, altitude),
                });
                path.modes.insert(path.custom_mode);
                path.points.push((
                    position,
                    PathPoint {
                        altitude,
                        custom_mode: path.custom_mode,
                    },
                ));
            }
        }
    }

    /// `None` for the out-of-range values senders use for "unknown" (zenith's downlink sends
    /// `i32::MAX`) and for 0/0, which is what autopilots send before their first fix.
    fn position(lat: i32, lon: i32) -> Option<Position> {
        let (lat, lon) = (f64::from(lat) / 10_000_000.0, f64::from(lon) / 10_000_000.0);
        let valid = lat.abs() <= 90.0 && lon.abs() <= 180.0 && (lat, lon) != (0.0, 0.0);
        valid.then(|| Position::new(lon, lat))
    }

    /// The center and zoom that show all `points` inside `rect`.
    fn fit_view(points: &[Position], rect: Rect) -> Option<(Position, f64)> {
        let first = mercator(*points.first()?);
        let (min, max) = points.iter().skip(1).fold((first, first), |(min, max), p| {
            let p = mercator(*p);
            (
                (min.0.min(p.0), min.1.min(p.1)),
                (max.0.max(p.0), max.1.max(p.1)),
            )
        });

        // Normalized mercator coordinates span 256 px at zoom 0.
        let span = ((max.0 - min.0) * 256.0, (max.1 - min.1) * 256.0);
        let room = rect.shrink(FIT_PADDING).size().max(Vec2::splat(1.0));
        let zoom = (f64::from(room.x) / span.0)
            .min(f64::from(room.y) / span.1)
            .log2()
            .clamp(FIT_MIN_ZOOM, FIT_MAX_ZOOM);

        let (x, y) = (f64::midpoint(min.0, max.0), f64::midpoint(min.1, max.1));
        let lat = (std::f64::consts::PI * (1.0 - 2.0 * y))
            .sinh()
            .atan()
            .to_degrees();

        Some((Position::new(x * 360.0 - 180.0, lat), zoom))
    }

    fn overlay_frame(ui: &Ui) -> Frame {
        Frame::window(ui.style()).shadow(OVERLAY_SHADOW)
    }

    fn corner(ui: &mut Ui, rect: Rect, layout: Layout) -> Ui {
        ui.new_child(UiBuilder::new().max_rect(rect.shrink(10.0)).layout(layout))
    }

    fn controls_ui(&mut self, ui: &mut Ui, rect: Rect) {
        let mut left = Self::corner(ui, rect, Layout::left_to_right(Align::Min));
        Self::overlay_frame(&left).show(&mut left, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.visualization, Visualization::Altitude, "Altitude");
                ui.selectable_value(
                    &mut self.visualization,
                    Visualization::FlightMode,
                    "Flight Mode",
                );

                if ui.button("🗑").on_hover_text("Clear trails").clicked() {
                    self.system_paths.values_mut().for_each(SystemPath::clear);
                }

                if let Some(layer) = &self.mapbox {
                    ui.separator();
                    ui.selectable_value(&mut self.show_mapbox, false, "Map");
                    ui.selectable_value(
                        &mut self.show_mapbox,
                        true,
                        Self::style_label(layer.style),
                    );
                }
            });
        });

        let mut right = Self::corner(ui, rect, Layout::right_to_left(Align::Min));
        Self::overlay_frame(&right).show(&mut right, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(self.fit, "⛶")
                    .on_hover_text("Keep home, vehicles and the ground station in view")
                    .clicked()
                {
                    self.fit = !self.fit;
                }

                ui.separator();
                if ui.button("-").on_hover_text("Zoom out").clicked() {
                    let _ = self.memory.zoom_out();
                    self.fit = false;
                }
                if ui.button("+").on_hover_text("Zoom in").clicked() {
                    let _ = self.memory.zoom_in();
                    self.fit = false;
                }
            });
        });
    }

    fn attribution_ui(ui: &mut Ui, rect: Rect, attribution: &walkers::sources::Attribution) {
        let mut ui = Self::corner(ui, rect, Layout::bottom_up(Align::Max));
        Self::overlay_frame(&ui)
            .inner_margin(4)
            .show(&mut ui, |ui| {
                ui.horizontal(|ui| {
                    let logo = if ui.visuals().dark_mode {
                        &attribution.logo_light
                    } else {
                        &attribution.logo_dark
                    };
                    if let Some(logo) = logo {
                        ui.add(egui::Image::new(logo.clone()).max_height(14.0));
                    }
                    ui.hyperlink_to(RichText::new(attribution.text).small(), attribution.url);
                });
            });
    }

    fn scale_ui(ui: &mut Ui, projector: &Projector, center: Position) {
        let pixels_per_meter = f64::from(projector.scale_pixel_per_meter(center));
        if !pixels_per_meter.is_normal() {
            return;
        }

        // The longest round distance that fits in about 100 px.
        let max = 100.0 / pixels_per_meter;
        let magnitude = 10f64.powf(max.log10().floor());
        let meters = [5.0, 2.0, 1.0]
            .into_iter()
            .map(|step| step * magnitude)
            .find(|meters| *meters <= max)
            .unwrap_or(magnitude);
        let width = (meters * pixels_per_meter) as f32;

        let label = if meters >= 1000.0 {
            format!("{} km", meters / 1000.0)
        } else {
            format!("{meters} m")
        };

        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 8.0), Sense::hover());
            let stroke = Stroke::new(1.5, ui.visuals().text_color());
            ui.painter().line(
                vec![
                    rect.left_top(),
                    rect.left_bottom(),
                    rect.right_bottom(),
                    rect.right_top(),
                ],
                stroke,
            );
            ui.small(label);
        });
    }

    fn legend_ui(&self, ui: &mut Ui, behavior: &TreeBehavior, altitude_range: Option<(f64, f64)>) {
        match self.visualization {
            Visualization::Altitude => {
                let Some((min, max)) = altitude_range else {
                    return;
                };
                ui.horizontal(|ui| {
                    ui.small(format!("{min:.0} m"));
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(100.0, 8.0), Sense::hover());
                    let steps = 50u16;
                    for i in 0..steps {
                        let x =
                            |i: u16| rect.left() + rect.width() * f32::from(i) / f32::from(steps);
                        ui.painter().rect_filled(
                            Rect::from_x_y_ranges(x(i)..=x(i + 1), rect.y_range()),
                            0.0,
                            gradient_color(f64::from(i) / f64::from(steps - 1)),
                        );
                    }
                    ui.small(format!("{max:.0} m"));
                });
            }
            Visualization::FlightMode => {
                let modes: Vec<(u8, u32)> = self
                    .system_paths
                    .iter()
                    .filter(|(_, path)| path.points.len() >= 2)
                    .flat_map(|(s_id, path)| path.modes.iter().map(|mode| (*s_id, *mode)))
                    .collect();

                for (s_id, mode) in modes {
                    let name = behavior
                        .source
                        .system(s_id)
                        .and_then(|system| system.custom_mode_info(mode))
                        .map(|info| nadir_core::mode_name_string(&info.mode_name))
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| format!("0x{mode:02x}"));

                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::new(14.0, 8.0), Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, mode_color(mode));
                        ui.small(name);
                    });
                }
            }
        }
    }
}

impl PaneUi for MapPane {
    fn inset(&mut self, _ui: &mut egui::Ui) -> f32 {
        0.0
    }

    #[allow(clippy::too_many_lines)]
    fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        self.sync_mapbox(&behavior.map);

        let rect = ui.available_rect_before_wrap();
        let system_ids = behavior.source.known_system_ids();
        self.ingest(behavior, &system_ids);

        let gcs_position = behavior
            .map
            .show_gcs
            .then(|| Position::new(behavior.map.gcs_position.lon, behavior.map.gcs_position.lat));
        let fallback_center =
            Position::new(behavior.map.gcs_position.lon, behavior.map.gcs_position.lat);

        let active_system_id = if let View::System {
            system_id: s_id, ..
        } = behavior.active_view
        {
            Some(s_id)
        } else {
            None
        };
        let active_system =
            active_system_id.and_then(|system_id| behavior.source.system(system_id));

        let systems: Vec<System> = system_ids
            .iter()
            .filter_map(|id| behavior.source.system(*id))
            .collect();

        let vehicles: HashMap<u8, (&System, Position, GlobalPositionInt)> = systems
            .iter()
            .filter_map(|s| {
                let gps = s.last_message::<GlobalPositionInt>().ok()?;
                Some((s.system_id, (s, Self::position(gps.lat, gps.lon)?, gps)))
            })
            .collect();

        let homes: Vec<Position> = systems
            .iter()
            .filter_map(|s| s.last_message::<HomePosition>().ok())
            .filter_map(|home| Self::position(home.latitude, home.longitude))
            .collect();

        if self.fit {
            let points: Vec<Position> = vehicles
                .values()
                .map(|(_, pos, _)| *pos)
                .chain(homes.iter().copied())
                .chain(gcs_position)
                .collect();
            if let Some((center, zoom)) = Self::fit_view(&points, rect) {
                self.memory.center_at(center);
                let _ = self.memory.set_zoom(zoom);
            }
        }
        let fitted_zoom = self.memory.zoom();

        let label_background = ui.visuals().window_fill();
        let places = vehicles
            .iter()
            .map(|(s_id, (s, pos, gps))| {
                let mode = s
                    .current_mode_name()
                    .map(|name| format!("\n{name}"))
                    .unwrap_or_default();
                let alt = f64::from(gps.relative_alt) / 1000.0;
                let vz = f64::from(gps.vz) / -100.0;

                LabeledSymbol {
                    position: *pos,
                    label: format!("System 0x{s_id:02x}{mode}\n☁ {alt:.1} m\n↕ {vz:.1} m/s"),
                    symbol: Some(Symbol::Circle(s.icon().to_string())),
                    style: LabeledSymbolStyle {
                        symbol_size: 20.0,
                        label_color: ui.visuals().text_color(),
                        label_background: if Some(*s_id) == active_system_id
                            || active_system_id.is_none()
                        {
                            label_background
                        } else {
                            label_background.gamma_multiply(0.6)
                        },
                        ..Default::default()
                    },
                }
            })
            .collect();

        let headings = vehicles
            .values()
            .filter(|(_, _, gps)| gps.hdg != u16::MAX)
            .map(|(_, pos, gps)| (*pos, f32::from(gps.hdg).to_radians() / 100.0))
            .collect();

        let simple_place_style = LabeledSymbolStyle {
            symbol_size: 20.0,
            symbol_color: Color32::WHITE,
            symbol_background: Color32::BLACK,
            ..Default::default()
        };

        let altitude_range = self
            .system_paths
            .values()
            .filter(|p| p.points.len() >= 2)
            .filter_map(|p| p.altitude_range)
            .reduce(|(a_min, a_max), (b_min, b_max)| (a_min.min(b_min), a_max.max(b_max)));
        let (alt_min, alt_span) =
            altitude_range.map_or((0.0, 1.0), |(min, max)| (min, (max - min).max(1.0)));

        // Build path plugins for each system
        let gradient = self.gradient.clone();
        let vis = self.visualization;
        let paths: Vec<Path<'_, PathPoint>> = self
            .system_paths
            .values()
            .map(|p| &p.points)
            .filter(|p| p.len() >= 2)
            .map(|path_data| {
                let gradient = gradient.clone();
                Path {
                    values: path_data,
                    stroke_callback: Box::new(move |pt: &PathPoint| match vis {
                        Visualization::Altitude => {
                            let f = ((pt.altitude - alt_min) / alt_span).clamp(0.0, 1.0);
                            let i = (f * (gradient.len() - 1) as f64) as usize;
                            Stroke {
                                width: (1.5 + f * 4.0) as f32,
                                color: gradient[i],
                            }
                        }
                        Visualization::FlightMode => Stroke {
                            width: 3.0,
                            color: mode_color(pt.custom_mode),
                        },
                    }),
                }
            })
            .collect();

        let tiles = match self.mapbox.as_mut() {
            Some(layer) if self.show_mapbox => &mut layer.tiles,
            _ => &mut self.osm_tiles,
        };
        let attribution = tiles.attribution();

        let mut map = walkers::Map::new(Some(tiles.as_mut()), &mut self.memory, fallback_center);

        if !paths.is_empty() {
            map = map.with_plugin(PathPlugin::new(paths));
        }

        if let Some(gcs_pos) = gcs_position {
            map = map.with_plugin(Places::new(vec![LabeledSymbol {
                position: gcs_pos,
                symbol: Some(Symbol::Circle("📡".to_string())),
                label: String::new(),
                style: simple_place_style.clone(),
            }]));
        }

        map = map.with_plugin(Places::new(
            homes
                .iter()
                .map(|home| LabeledSymbol {
                    position: *home,
                    symbol: Some(Symbol::Circle("🏠".to_string())),
                    label: String::new(),
                    style: simple_place_style.clone(),
                })
                .collect(),
        ));

        if let Some(system) = &active_system {
            map = map.with_plugin(NavigationPlugin {
                system: system.clone(),
            });

            if let Some((_s, pos, ..)) = vehicles.get(&system.system_id)
                && let Some(gcs_pos) = gcs_position
            {
                map = map.with_plugin(LinePlugin {
                    a: gcs_pos,
                    b: *pos,
                    color: Color32::BLACK,
                });
            }

            if let Some(target_pos) = system
                .last_message::<PositionTargetGlobalInt>()
                .ok()
                .and_then(|target| Self::position(target.lat_int, target.lon_int))
            {
                if let Some((_s, pos, ..)) = vehicles.get(&system.system_id) {
                    map = map.with_plugin(LinePlugin {
                        a: *pos,
                        b: target_pos,
                        color: Color32::PURPLE.linear_multiply(1.5),
                    });
                }

                map = map.with_plugin(Places::new(vec![LabeledSymbol {
                    position: target_pos,
                    symbol: Some(Symbol::Circle("🏁".to_string())),
                    label: String::new(),
                    style: simple_place_style,
                }]));
            }
        }

        map = map.with_plugin(HeadingPlugin {
            headings,
            radius: 10.0,
            color: Color32::BLACK.gamma_multiply(0.8),
        });
        map = map.with_plugin(Places::new(places));

        let response = ui.add(map);

        if self.fit && (response.dragged() || self.memory.zoom() != fitted_zoom) {
            self.fit = false;
        }

        let center = self.memory.detached().unwrap_or(fallback_center);
        self.center = Some(center);

        self.controls_ui(ui, response.rect);
        Self::attribution_ui(ui, response.rect, &attribution);

        let projector = Projector::new(response.rect, &self.memory, fallback_center);
        let mut corner = Self::corner(ui, response.rect, Layout::bottom_up(Align::Min));
        Self::overlay_frame(&corner)
            .inner_margin(4)
            .show(&mut corner, |ui| {
                // Bottom-up, like the corner: the scale sits below the legend.
                Self::scale_ui(ui, &projector, center);
                self.legend_ui(ui, behavior, altitude_range);
            });
    }
}

/// `position` in Web Mercator, normalized to 0..1 from the north-west corner.
fn mercator(position: Position) -> (f64, f64) {
    let lat = position.y().clamp(-85.05, 85.05).to_radians();
    let x = (position.x() + 180.0) / 360.0;
    let y = (1.0 - lat.tan().asinh() / std::f64::consts::PI) / 2.0;
    (x, y)
}

/// Matplotlib's plasma colormap, sampled at nine evenly spaced points.
const PLASMA: [Color32; 9] = [
    Color32::from_rgb(0x0d, 0x08, 0x87),
    Color32::from_rgb(0x4c, 0x02, 0xa1),
    Color32::from_rgb(0x7e, 0x03, 0xa8),
    Color32::from_rgb(0xa9, 0x23, 0x95),
    Color32::from_rgb(0xcc, 0x47, 0x78),
    Color32::from_rgb(0xe5, 0x6b, 0x5d),
    Color32::from_rgb(0xf8, 0x95, 0x40),
    Color32::from_rgb(0xfd, 0xc5, 0x27),
    Color32::from_rgb(0xf0, 0xf9, 0x21),
];

fn gradient_color(t: f64) -> Color32 {
    let scaled = t.clamp(0.0, 1.0) * (PLASMA.len() - 1) as f64;
    let i = (scaled.floor() as usize).min(PLASMA.len() - 2);
    PLASMA[i].lerp_to_gamma(PLASMA[i + 1], (scaled - i as f64) as f32)
}

fn build_gradient(steps: usize) -> Vec<Color32> {
    (0..steps)
        .map(|i| gradient_color(i as f64 / (steps - 1) as f64))
        .collect()
}

struct Path<'a, T> {
    values: &'a [(Position, T)],
    stroke_callback: Box<dyn Fn(&T) -> Stroke>,
}

struct PathPlugin<'a, T> {
    paths: Vec<Path<'a, T>>,
}

impl<'a, T> PathPlugin<'a, T> {
    fn new(paths: Vec<Path<'a, T>>) -> Self {
        Self { paths }
    }
}

impl<T> Plugin for PathPlugin<'_, T> {
    fn run(
        self: Box<Self>,
        ui: &mut Ui,
        _response: &egui::Response,
        projector: &Projector,
        _memory: &MapMemory,
    ) {
        for p in &self.paths {
            let screen_positions: Vec<_> = p
                .values
                .iter()
                .map(|(pos, val)| (projector.project(*pos).to_pos2(), val))
                .collect();
            for segment in screen_positions.windows(2) {
                ui.painter().line_segment(
                    [segment[0].0, segment[1].0],
                    (p.stroke_callback)(segment[0].1),
                );
            }
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Visualization {
    Altitude,
    FlightMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_positions_are_rejected() {
        assert!(MapPane::position(i32::MAX, i32::MAX).is_none());
        assert!(MapPane::position(0, 0).is_none());
        assert!(MapPane::position(498_541_820, 85_924_050).is_some());
    }

    #[test]
    fn a_single_point_fits_at_the_closest_zoom() {
        let p = Position::new(8.59, 49.86);
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(800.0));
        let (center, zoom) = MapPane::fit_view(&[p], rect).unwrap();

        assert_eq!(zoom, FIT_MAX_ZOOM);
        assert!((center.x() - p.x()).abs() < 1e-9 && (center.y() - p.y()).abs() < 1e-9);
    }

    #[test]
    fn two_points_fit_inside_the_padded_rect() {
        let (a, b) = (Position::new(8.50, 49.80), Position::new(8.70, 49.90));
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 600.0));
        let (center, zoom) = MapPane::fit_view(&[a, b], rect).unwrap();

        let world = 256.0 * 2f64.powf(zoom);
        let (ma, mb) = (mercator(a), mercator(b));
        let width = (mb.0 - ma.0).abs() * world;
        let height = (mb.1 - ma.1).abs() * world;
        let room = rect.shrink(FIT_PADDING).size();

        assert!(width <= f64::from(room.x) + 1e-6 && height <= f64::from(room.y) + 1e-6);
        assert!(
            (width - f64::from(room.x)).abs() < 1e-6 || (height - f64::from(room.y)).abs() < 1e-6,
            "neither side is filled at zoom {zoom}"
        );
        assert!(center.y() > 49.80 && center.y() < 49.90);
    }
}
