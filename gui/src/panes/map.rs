//! Contains our map widget, based on the walkers crate.

#![allow(dead_code)]
#![allow(unused)]

use eframe::egui;
use egui::{Color32, CornerRadius, Frame, Rect, Stroke, Style, Ui, Vec2, Widget};
use mavspec::rust::dialects::common::enums::MavCmd;
use walkers::{
    HttpOptions, HttpTiles, MapMemory, Plugin, Position, Projector,
    extras::{LabeledSymbol, LabeledSymbolStyle, Place, Places, Symbol},
};

use crate::{panes::TreeBehavior, views::View, widgets::FalseHorizon};

pub struct MapPane {
    osm_tiles: HttpTiles,
    mapbox_tiles: Option<HttpTiles>,
    memory: MapMemory,
    satellite: bool,
    position_source: PositionSource,
    visualization: Visualization,
    show_gizmos: bool,
    gradient_lookup: Vec<Color32>,
    //estimated_positions: Vec<(Position, (f64, Vector3<f32>, FlightMode, f32))>,
    //gps_positions: Vec<(Position, (f64, Vector3<f32>, FlightMode, f32))>,
    cached_state: Option<(f64, usize)>,
}

struct NavigationPlugin {
    system: core::System,
}

impl walkers::Plugin for NavigationPlugin {
    fn run(
        self: Box<Self>,
        ui: &mut Ui,
        response: &egui::Response,
        projector: &Projector,
        map_memory: &MapMemory,
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

impl MapPane {
    pub fn new(ctx: &egui::Context, mapbox_access_token: Option<String>) -> Self {
        let osm_tiles = HttpTiles::with_options(
            walkers::sources::OpenStreetMap,
            Self::http_options(),
            ctx.to_owned(),
        );

        // We only show the mapbox map if we have an access token
        let mapbox_access_token =
            mapbox_access_token.or(option_env!("MAPBOX_ACCESS_TOKEN").map(|s| s.to_string()));
        let mapbox_tiles = mapbox_access_token.map(|t| {
            HttpTiles::with_options(
                walkers::sources::Mapbox {
                    style: walkers::sources::MapboxStyle::Satellite,
                    access_token: t.to_string(),
                    high_resolution: true,
                },
                Self::http_options(),
                ctx.to_owned(),
            )
        });

        // We default to satellite view if we have one.
        let satellite = mapbox_tiles.is_some();

        let gradient_lookup = (0..=1000)
            //.map(|i| colorgrad::sinebow().at((i as f64) / 1000.0).to_rgba8())
            //.map(|color| Color32::from_rgb(color[0], color[1], color[2]))
            .map(|color| Color32::RED)
            .collect();

        Self {
            osm_tiles,
            mapbox_tiles,
            memory: MapMemory::default(),
            satellite,
            position_source: PositionSource::Gps,
            visualization: Visualization::Altitude,
            show_gizmos: true,
            gradient_lookup,
            //estimated_positions: Vec::new(),
            //gps_positions: Vec::new(),
            cached_state: None,
        }
    }

    fn http_options() -> HttpOptions {
        // We don't cache anything on web assembly
        #[cfg(target_arch = "wasm32")]
        let cache_path = None;

        // On Android, we just hardcode the path for now. If we wanted to do it properly, we'd
        // have to request a path and pass it to our code via the JNI.
        #[cfg(target_os = "android")]
        let cache_path = Some(std::path::PathBuf::from(
            "/data/user/0/space.tudsat.sam/cache",
        ));

        // On other platforms, we store map tiles on-disk
        //#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
        //let cache_path = Some(
        //    ProjectDirs::from("space", "tudsat", "rapid-control")
        //        .unwrap()
        //        .cache_dir()
        //        .into(),
        //);

        let cache_path = None;

        HttpOptions {
            cache: cache_path,
            ..Default::default()
        }
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let tiles = match self.mapbox_tiles.as_mut() {
            Some(tiles) if self.satellite => tiles,
            _ => &mut self.osm_tiles,
        };

        let detached_pos = self.memory.detached();

        let rect = ui.clip_rect();

        //let position = self
        //    .vehicle_position
        //    .map(|(pos, ..)| pos)
        //    .unwrap_or(Position::new(8.68519, 49.861445));
        //let gradient_lookup = self.state.gradient_lookup.clone();
        //let pos_source = self.state.position_source;

        //let vis_plugin = match self.state.visualization {
        //    Visualization::Altitude => Path {
        //        values: &self.vehicle_positions,
        //        stroke_callback: Box::new(move |(alt, _att, _fm, _var)| {
        //            let f = alt / GRADIENT_MAX_ALT;
        //            let i = (f * (gradient_lookup.len() as f64)) as usize;
        //            Stroke {
        //                width: (1.0 + (alt / GRADIENT_MAX_ALT) * 10.0) as f32,
        //                color: gradient_lookup[usize::min(i, gradient_lookup.len() - 1)],
        //            }
        //        }),
        //    },
        //    Visualization::FlightMode => Path {
        //        values: &self.vehicle_positions,
        //        stroke_callback: Box::new(move |(alt, _att, fm, _var)| Stroke {
        //            width: (1.0 + (alt / GRADIENT_MAX_ALT) * 10.0) as f32,
        //            color: fm.color(),
        //        }),
        //    },
        //    Visualization::Attitude => Path {
        //        values: &self.vehicle_positions,
        //        stroke_callback: Box::new(move |(alt, att, _fm, _var)| {
        //            let color = Color32::from_rgb(
        //                (256.0 * (att.x + 1.0) / 2.0) as u8,
        //                (256.0 * (att.y + 1.0) / 2.0) as u8,
        //                (256.0 * (att.z + 1.0) / 2.0) as u8,
        //            );
        //            Stroke {
        //                width: (1.0 + (alt / GRADIENT_MAX_ALT) * 10.0) as f32,
        //                color,
        //            }
        //        }),
        //    },
        //    Visualization::Uncertainty => Path {
        //        values: &self.vehicle_positions,
        //        stroke_callback: Box::new(move |(alt, _att, _fm, var)| {
        //            let f = if pos_source == PositionSource::Estimate {
        //                f32::min(*var, 5.0) / 5.0
        //            } else {
        //                var / 10.00
        //            };
        //            let i = ((0.3 - f64::min(f as f64, 1.0) * 0.3) * (gradient_lookup.len() as f64))
        //                as usize;
        //            Stroke {
        //                width: (1.0 + (alt / GRADIENT_MAX_ALT) * 10.0) as f32,
        //                color: gradient_lookup[usize::min(i, gradient_lookup.len() - 1)],
        //            }
        //        }),
        //    },
        //};

        // TODO
        let center_position = Position::new(-8.292362108248733, 39.394546258787685);

        let system_ids = behavior.core.known_system_ids();
        let systems = system_ids.iter().filter_map(|id| behavior.core.system(*id));
        let active_system_id = if let View::System(s_id) = behavior.active_view {
            Some(s_id)
        } else {
            None
        };

        let active_system = active_system_id
            .map(|system_id| behavior.core.system(system_id))
            .flatten();

        let places = systems
            .filter_map(|s| {
                s.last_global_position_int().unwrap_or_default().map(|gps| {
                    let pos = Position::new(
                        (gps.lon as f64) / 10_000_000.0,
                        (gps.lat as f64) / 10_000_000.0,
                    );

                    LabeledSymbol {
                        position: pos,
                        //Position::new(-8.292362108248733, 39.394546258787685),
                        label: format!(
                            "System 0x{:02x}\n☁ {}m\n↕ {}m/s",
                            s.system_id,
                            gps.alt as f64 / 1000.0,
                            gps.vz as f64 / -100.0,
                        ),
                        symbol: Some(Symbol::Circle(s.icon().to_string())),
                        style: LabeledSymbolStyle {
                            symbol_size: 20.0,
                            label_background: if Some(s.system_id) == active_system_id
                                || active_system_id == None
                            {
                                ui.style().visuals.window_fill()
                            } else {
                                ui.style().visuals.window_fill().gamma_multiply(0.6)
                            },
                            ..Default::default()
                        },
                    }
                })
            })
            .collect();

        let mut map = walkers::Map::new(Some(tiles), &mut self.memory, center_position)
            //.with_plugin(PathPlugin::new(vec![vis_plugin]))
            // TODO: configurable GCS position
            .with_plugin(Places::new(vec![LabeledSymbol {
                position: Position::new(-8.292362108248733, 39.394546258787685),
                symbol: Some(Symbol::Circle("📡".to_string())),
                label: "".to_string(),
                style: LabeledSymbolStyle {
                    symbol_size: 20.0,
                    symbol_color: Color32::WHITE,
                    symbol_background: Color32::BLACK,
                    ..Default::default()
                },
            }]))
            .with_plugin(Places::new(places));

        if let Some(system) = &active_system {
            map = map.with_plugin(NavigationPlugin {
                system: system.clone(),
            })
        }

        let response = ui.add(map);

        //if self.state.show_gizmos {
        //    if let Some(q) = self.orientation {
        //        let viewport = ui.clip_rect();

        //        // Fun type conversion bullshit
        //        let rotation: mint::Quaternion<f64> = q.cast::<f64>().into();
        //        let rotation: transform_gizmo_egui::mint::Quaternion<f64> = rotation.into();

        //        let view_matrix =
        //            DMat4::look_at_rh(DVec3::new(0., 0., 1.), DVec3::ZERO, DVec3::new(0., 1., 0.));
        //        let projection_matrix = DMat4::orthographic_rh(
        //            viewport.left() as f64,
        //            viewport.right() as f64,
        //            -viewport.bottom() as f64,
        //            -viewport.top() as f64,
        //            0.1,
        //            1000.0,
        //        );

        //        // We use viewport pixel coordinates (obtained from the Map projector)
        //        // for the rendering of the gizmo, but we need to invert the y axis,
        //        // since screen coordinates are Y down
        //        let projector = Projector::new(viewport, &self.state.memory, position);
        //        let viewport_pos = projector.project(position);
        //        let translation = DVec3::new(viewport_pos.x as f64, -viewport_pos.y as f64, 0.0);
        //        let transform =
        //            Transform::from_scale_rotation_translation(DVec3::ONE, rotation, translation);

        //        let visuals = GizmoVisuals {
        //            inactive_alpha: 1.0,
        //            highlight_alpha: 1.0,
        //            gizmo_size: 50.0,
        //            ..Default::default()
        //        };

        //        let config = GizmoConfig {
        //            viewport,
        //            view_matrix: view_matrix.into(),
        //            projection_matrix: projection_matrix.into(),
        //            modes: GizmoMode::all_translate(),
        //            orientation: transform_gizmo_egui::GizmoOrientation::Local,
        //            visuals,
        //            ..Default::default()
        //        };

        //        Gizmo::new(config).interact(ui, &[transform]);
        //    }
        //}

        let horizon_w = f32::max(150.0, f32::min(350.0, rect.width() / 6.0));
        let horizon_rect = Rect::from_two_pos(
            rect.left_top() + Vec2::new(10.0, 10.0),
            rect.left_top() + Vec2::new(10.0 + horizon_w, 10.0 + horizon_w + 50.0),
        );
        if let Some(system) = &active_system {
            ui.place(horizon_rect, |ui: &mut egui::Ui| {
                Frame::dark_canvas(ui.style())
                    .corner_radius(CornerRadius::ZERO.at_least(10))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(horizon_rect.width());
                            ui.set_height(horizon_rect.height());
                            ui.add_sized(horizon_rect.size(), FalseHorizon::new(system));
                        });
                    })
                    .response
            });
        }

        //// Panel for resetting map to vehicle position
        //let reset_rect = Rect::from_two_pos(
        //    rect.right_bottom() + Vec2::new(-10.0, -10.0),
        //    rect.right_bottom() + Vec2::new(-40.0, -40.0),
        //);
        //ui.put(reset_rect, |ui: &mut egui::Ui| {
        //    Frame::window(ui.style())
        //        .show(ui, |ui| {
        //            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
        //                let detached_pos = self.state.memory.detached();
        //                let pos = detached_pos.or(self.vehicle_position.map(|(p, ..)| p));
        //                let coords = pos.map(|p| format!("{:.6},{:.6}", p.y(), p.x()));

        //                ui.add_enabled_ui(detached_pos.is_some(), |ui| {
        //                    if ui.button("⌖").clicked() {
        //                        self.state.memory.follow_my_position();
        //                    }
        //                });

        //                ui.add_enabled_ui(coords.is_some(), |ui| {
        //                    if ui.button("📋").clicked() {
        //                        ui.ctx().copy_text(coords.clone().unwrap_or_default());
        //                    }
        //                });

        //                if detached_pos.is_some() {
        //                    ui.monospace(coords.unwrap_or_default());
        //                }
        //            })
        //            .response
        //        })
        //        .response
        //});

        //// Panel for selecting path visualizations
        //let map_type_rect = Rect::from_two_pos(
        //    rect.left_top() + Vec2::new(10.0, 10.0),
        //    rect.left_top() + Vec2::new(100.0, 40.0),
        //);
        //ui.put(map_type_rect, |ui: &mut egui::Ui| {
        //    Frame::window(ui.style())
        //        .show(ui, |ui| {
        //            ui.horizontal(|ui| {
        //                ui.selectable_value(
        //                    &mut self.state.position_source,
        //                    PositionSource::Estimate,
        //                    "🗠",
        //                );
        //                ui.selectable_value(
        //                    &mut self.state.position_source,
        //                    PositionSource::Gps,
        //                    "🌍",
        //                );
        //                ui.separator();
        //                ui.selectable_value(
        //                    &mut self.state.visualization,
        //                    Visualization::Altitude,
        //                    "⬍",
        //                );
        //                ui.selectable_value(
        //                    &mut self.state.visualization,
        //                    Visualization::FlightMode,
        //                    "🏷",
        //                );
        //                ui.selectable_value(
        //                    &mut self.state.visualization,
        //                    Visualization::Attitude,
        //                    "🔃",
        //                );
        //                ui.selectable_value(
        //                    &mut self.state.visualization,
        //                    Visualization::Uncertainty,
        //                    "⁉",
        //                );
        //            });
        //        })
        //        .response
        //});

        //// Panel for switching between gizmos and position tags
        //let gizmo_rect = Rect::from_two_pos(
        //    rect.right_top() + Vec2::new(-10.0, 10.0),
        //    rect.right_top() + Vec2::new(-100.0, 40.0),
        //);
        //ui.put(gizmo_rect, |ui: &mut egui::Ui| {
        //    Frame::window(ui.style())
        //        .show(ui, |ui| {
        //            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
        //                ui.selectable_value(&mut self.state.show_gizmos, false, "📋");
        //                ui.selectable_value(&mut self.state.show_gizmos, true, "🔃");
        //            });
        //        })
        //        .response
        //});

        // TODO: attribution

        //response
    }
}

// use crate::Backend;
// use crate::settings::AppSettings;
// use crate::utils::telemetry_ext::ColorExt;

const GRADIENT_MAX_ALT: f64 = 10000.0;

pub struct Path<'a, T> {
    values: &'a Vec<(Position, T)>,
    stroke_callback: Box<dyn Fn(&T) -> egui::Stroke>,
}

pub struct PathPlugin<'a, T> {
    paths: Vec<Path<'a, T>>,
}

impl<'a, T> PathPlugin<'a, T> {
    pub fn new(paths: Vec<Path<'a, T>>) -> Self {
        Self { paths }
    }
}

impl<'a, T> Plugin for PathPlugin<'a, T> {
    fn run(
        self: Box<Self>,
        ui: &mut Ui,
        _response: &egui::Response,
        projector: &walkers::Projector,
        _memory: &walkers::MapMemory,
    ) {
        for p in &self.paths {
            let screen_positions: Vec<_> = p
                .values
                .iter()
                .map(|(p, val)| (projector.project(*p).to_pos2(), val))
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
enum PositionSource {
    Estimate,
    Gps,
}

#[derive(PartialEq, Clone, Copy)]
enum Visualization {
    Altitude,
    FlightMode,
    Attitude,
    Uncertainty,
}
