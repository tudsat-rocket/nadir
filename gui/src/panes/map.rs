//! Contains our map widget, based on the walkers crate.

use core::System;
use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, Utc};

use eframe::egui;
use egui::{Color32, Frame, Pos2, Rect, Shape, Stroke, Ui, Vec2};
use mavspec::rust::dialects::common::messages::{
    GlobalPositionInt, Heartbeat, HomePosition, PositionTargetGlobalInt,
};
use walkers::{
    HttpOptions, HttpTiles, MapMemory, Plugin, Position, Projector,
    extras::{LabeledSymbol, LabeledSymbolStyle, Places, Symbol},
};

use crate::{
    colors::mode_color,
    panes::{PaneUi, TreeBehavior},
    views::View,
};

#[derive(Clone, Copy)]
struct PathPoint {
    altitude: f64,
    custom_mode: u32,
}

#[derive(Default)]
struct SystemPath {
    points: Vec<(Position, PathPoint)>,
    last_gps: Option<DateTime<Utc>>,
    last_heartbeat: Option<DateTime<Utc>>,
    /// Heartbeats newer than the last appended GPS point, waiting to be joined once position
    /// messages catch up.
    pending_heartbeats: VecDeque<(DateTime<Utc>, u32)>,
    custom_mode: u32,
}

pub struct MapPane {
    osm_tiles: HttpTiles,
    mapbox_tiles: Option<HttpTiles>,
    memory: MapMemory,
    satellite: bool,
    visualization: Visualization,
    gradient: Vec<Color32>,
    system_paths: HashMap<u8, SystemPath>,
}

struct NavigationPlugin {
    system: core::System,
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

impl MapPane {
    pub fn new(ctx: &egui::Context, mapbox_access_token: Option<String>) -> Self {
        let osm_tiles = HttpTiles::with_options(
            walkers::sources::OpenStreetMap,
            Self::http_options(),
            ctx.to_owned(),
        );

        // We only show the mapbox map if we have an access token
        let mapbox_access_token = mapbox_access_token
            .or(option_env!("MAPBOX_ACCESS_TOKEN").map(std::string::ToString::to_string));
        let mapbox_tiles = mapbox_access_token.map(|t| {
            HttpTiles::with_options(
                walkers::sources::Mapbox {
                    style: walkers::sources::MapboxStyle::Satellite,
                    access_token: t.clone(),
                    high_resolution: true,
                },
                Self::http_options(),
                ctx.to_owned(),
            )
        });

        // We default to satellite view if we have one.
        let satellite = mapbox_tiles.is_some();

        let gradient = build_gradient(1001);

        Self {
            osm_tiles,
            mapbox_tiles,
            memory: MapMemory::default(),
            satellite,
            visualization: Visualization::Altitude,
            gradient,
            system_paths: HashMap::new(),
        }
    }

    // TODO: cache tiles under the ProjectDirs cache_dir off the browser, where `cache` is ignored
    fn http_options() -> HttpOptions {
        HttpOptions::default()
    }
}

impl PaneUi for MapPane {
    fn inset(&mut self, _ui: &mut egui::Ui) -> f32 {
        0.0
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let tiles = match self.mapbox_tiles.as_mut() {
            Some(tiles) if self.satellite => tiles,
            _ => &mut self.osm_tiles,
        };

        let rect = ui.clip_rect();

        let system_ids = behavior.source.known_system_ids();

        // Append newly arrived positions to each system's cached path.
        for s_id in &system_ids {
            let Some(system) = behavior.source.system(*s_id) else {
                continue;
            };
            let count = system.message_count::<GlobalPositionInt>();
            let path = self.system_paths.entry(*s_id).or_default();
            if count == path.points.len() {
                continue;
            }

            let new_gps = system.messages_since::<GlobalPositionInt>(path.last_gps, None);
            let new_heartbeats = system.messages_since::<Heartbeat>(path.last_heartbeat, None);

            for (ts, hb) in new_heartbeats {
                path.pending_heartbeats.push_back((ts, hb.custom_mode));
                path.last_heartbeat = Some(ts);
            }

            for (ts, gps) in new_gps {
                // Advance the flight mode to the last heartbeat at or before this GPS timestamp;
                // later ones stay pending for future points.
                while let Some((hb_ts, mode)) = path.pending_heartbeats.front().copied() {
                    if hb_ts > ts {
                        break;
                    }
                    path.custom_mode = mode;
                    path.pending_heartbeats.pop_front();
                }

                let pos = Position::new(
                    f64::from(gps.lon) / 10_000_000.0,
                    f64::from(gps.lat) / 10_000_000.0,
                );
                path.points.push((
                    pos,
                    PathPoint {
                        altitude: f64::from(gps.relative_alt) / 1000.0,
                        custom_mode: path.custom_mode,
                    },
                ));
                path.last_gps = Some(ts);
            }
        }

        // TODO: configurable GCS position
        //let gcs_position = Some(Position::new(-8.292362108248733, 39.394546258787685));
        let gcs_position = Some(Position::new(8.592_405, 49.855_982));

        // TODO
        #[allow(clippy::unnecessary_literal_unwrap)]
        let center_position = gcs_position.unwrap();

        let systems = system_ids
            .iter()
            .filter_map(|id| behavior.source.system(*id));
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

        let system_positions: HashMap<u8, (System, Position, f64, f64)> = systems
            .filter_map(|s| {
                s.last_message::<GlobalPositionInt>().ok().map(|gps| {
                    let s_id = s.system_id;
                    let pos = Position::new(
                        f64::from(gps.lon) / 10_000_000.0,
                        f64::from(gps.lat) / 10_000_000.0,
                    );
                    (
                        s_id,
                        (
                            s,
                            pos,
                            f64::from(gps.alt) / 1000.0,
                            f64::from(gps.vz) / -100.0,
                        ),
                    )
                })
            })
            .collect();

        let places = system_positions
            .iter()
            .map(|(s_id, (s, pos, alt, vz))| {
                LabeledSymbol {
                    position: *pos,
                    //Position::new(-8.292362108248733, 39.394546258787685),
                    label: format!("System 0x{s_id:02x}\n☁ {alt}m\n↕ {vz}m/s"),
                    symbol: Some(Symbol::Circle(s.icon().to_string())),
                    style: LabeledSymbolStyle {
                        symbol_size: 20.0,
                        label_background: if Some(s.system_id) == active_system_id
                            || active_system_id.is_none()
                        {
                            ui.style().visuals.window_fill()
                        } else {
                            ui.style().visuals.window_fill().gamma_multiply(0.6)
                        },
                        ..Default::default()
                    },
                }
            })
            .collect();

        let simple_place_style = LabeledSymbolStyle {
            symbol_size: 20.0,
            symbol_color: Color32::WHITE,
            symbol_background: Color32::BLACK,
            ..Default::default()
        };

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
                            let f = (pt.altitude / GRADIENT_MAX_ALT).clamp(0.0, 1.0);
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

        let mut map = walkers::Map::new(Some(tiles), &mut self.memory, center_position);

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

        if let Some(system) = &active_system {
            map = map.with_plugin(NavigationPlugin {
                system: system.clone(),
            });

            if let Some((_s, pos, ..)) = system_positions.get(&system.system_id)
                && let Some(gcs_pos) = gcs_position
            {
                map = map.with_plugin(LinePlugin {
                    a: gcs_pos,
                    b: *pos,
                    color: Color32::BLACK,
                });
            }

            if let Ok(target) = system.last_message::<PositionTargetGlobalInt>() {
                if let Some((_s, pos, ..)) = system_positions.get(&system.system_id) {
                    map = map.with_plugin(LinePlugin {
                        a: *pos,
                        b: Position::new(
                            f64::from(target.lon_int) / 10_000_000.0,
                            f64::from(target.lat_int) / 10_000_000.0,
                        ),
                        color: Color32::PURPLE.linear_multiply(1.5),
                    });
                }

                map = map.with_plugin(Places::new(vec![LabeledSymbol {
                    position: Position::new(
                        f64::from(target.lon_int) / 10_000_000.0,
                        f64::from(target.lat_int) / 10_000_000.0,
                    ),
                    symbol: Some(Symbol::Circle("🏁".to_string())),
                    label: String::new(),
                    style: simple_place_style.clone(),
                }]));
            }

            if let Ok(home) = system.last_message::<HomePosition>() {
                map = map.with_plugin(Places::new(vec![LabeledSymbol {
                    position: Position::new(
                        f64::from(home.longitude) / 10_000_000.0,
                        f64::from(home.latitude) / 10_000_000.0,
                    ),
                    symbol: Some(Symbol::Circle("🏠".to_string())),
                    label: String::new(),
                    style: simple_place_style,
                }]));
            }
        }

        map = map.with_plugin(Places::new(places));

        let _response = ui.add(map);

        // Visualization selector overlay
        let vis_rect = Rect::from_two_pos(
            rect.left_top() + Vec2::new(10.0, 10.0),
            rect.left_top() + Vec2::new(160.0, 40.0),
        );
        ui.put(vis_rect, |ui: &mut egui::Ui| {
            Frame::window(ui.style())
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut self.visualization,
                            Visualization::Altitude,
                            "Altitude",
                        );
                        ui.selectable_value(
                            &mut self.visualization,
                            Visualization::FlightMode,
                            "Flight Mode",
                        );
                    });
                })
                .response
        });
    }
}

const GRADIENT_MAX_ALT: f64 = 10000.0;

/// Blue -> Cyan -> Green -> Yellow -> Red heat-map
fn gradient_color(t: f64) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let (r, g, b) = if t < 0.25 {
        let s = t / 0.25;
        (0.0, s, 1.0)
    } else if t < 0.5 {
        let s = (t - 0.25) / 0.25;
        (0.0, 1.0, 1.0 - s)
    } else if t < 0.75 {
        let s = (t - 0.5) / 0.25;
        (s, 1.0, 0.0)
    } else {
        let s = (t - 0.75) / 0.25;
        (1.0, 1.0 - s, 0.0)
    };
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
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
