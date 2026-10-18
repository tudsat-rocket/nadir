use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use nadir_core::{Origin, SessionAddr, SessionSummary, Source};

use crate::colors::{COLOR_INDICATOR_GOOD, readable};
use crate::views::SourceId;
use crate::widgets::{Readout, ago};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const ENDED_LIMIT: usize = 10;

type Listing = Result<Vec<SessionSummary>, String>;

/// The sessions the configured `meridian` service lists, polled for as long as one is configured.
#[derive(Default)]
pub struct Sessions {
    server: Option<String>,
    listing: Option<Listing>,
    /// Every id listed so far. `None` until the first listing, whose sessions are not new to anyone.
    seen: Option<HashSet<String>>,
    /// Published from this station.
    own: HashSet<String>,
    joined: HashSet<String>,
    arrived: Arc<Mutex<Option<Listing>>>,
    in_flight: bool,
    last_fetch: Option<Instant>,
}

impl Sessions {
    /// Keeps the listing fresh, returning the sessions others started since the last one.
    pub fn poll(
        &mut self,
        ctx: &egui::Context,
        server: Option<&str>,
        live: &Source,
        logs: &BTreeMap<SourceId, Source>,
    ) -> Vec<SessionSummary> {
        if self.server.as_deref() != server {
            *self = Self {
                server: server.map(str::to_owned),
                ..Self::default()
            };
        }
        let Some(server) = server else {
            return Vec::new();
        };

        self.own = live
            .known_system_ids()
            .iter()
            .filter_map(|id| live.system(*id)?.sharing())
            .map(|(addr, _)| addr.id)
            .collect();
        self.joined = logs
            .values()
            .filter_map(|source| match &source.origin {
                Origin::Stream(progress) => Some(progress.session.clone()),
                Origin::Live | Origin::Log(_) => None,
            })
            .collect();

        let mut started = Vec::new();

        if let Some(mut listing) = self.arrived.lock().unwrap().take() {
            self.in_flight = false;

            if let Ok(sessions) = &mut listing {
                sessions.sort_by_key(|s| (!s.live(), s.idle));

                if let Some(seen) = &self.seen {
                    started = sessions
                        .iter()
                        .filter(|s| s.live() && !seen.contains(&s.id) && !self.own.contains(&s.id))
                        .cloned()
                        .collect();
                }
                self.seen
                    .get_or_insert_default()
                    .extend(sessions.iter().map(|s| s.id.clone()));
            }

            self.listing = Some(listing);
        }

        ctx.request_repaint_after(REFRESH_INTERVAL);

        if !self.in_flight
            && self
                .last_fetch
                .is_none_or(|at| at.elapsed() >= REFRESH_INTERVAL)
        {
            self.fetch(ctx, server);
        }

        started
    }

    pub fn live_count(&self) -> usize {
        match &self.listing {
            Some(Ok(sessions)) => sessions.iter().filter(|s| s.live()).count(),
            _ => 0,
        }
    }

    /// The listing, live sessions first, returning the one whose Join was clicked.
    pub fn ui(&self, ui: &mut egui::Ui) -> Option<SessionAddr> {
        let Some(server) = &self.server else {
            ui.weak("Set a sharing server in Preferences.");
            return None;
        };

        let sessions = match &self.listing {
            None => {
                ui.spinner();
                return None;
            }
            Some(Err(e)) => {
                ui.weak(format!("{server}: {e}"));
                return None;
            }
            Some(Ok(sessions)) if sessions.is_empty() => {
                ui.weak("No sessions.");
                return None;
            }
            Some(Ok(sessions)) => sessions,
        };

        let mut picked = None;
        let live = sessions.iter().filter(|s| s.live()).count();

        ui.horizontal(|ui| {
            ui.add_space(5.0);
            egui::Grid::new("sessions")
                .min_col_width(0.0)
                .show(ui, |ui| {
                    for session in sessions.iter().take(live + ENDED_LIMIT) {
                        if self.row(ui, session) {
                            picked = Some(SessionAddr {
                                server: server.clone(),
                                id: session.id.clone(),
                            });
                        }
                        ui.end_row();
                    }
                });
        });

        picked
    }

    /// One grid row, returning whether Join was clicked.
    fn row(&self, ui: &mut egui::Ui, session: &SessionSummary) -> bool {
        if session.live() {
            ui.label(egui::RichText::new("🔴").color(readable(COLOR_INDICATOR_GOOD, ui.visuals())))
                .on_hover_text("Live");
        } else {
            ui.weak("⏹").on_hover_text("Ended");
        }

        match session.parts() {
            Some((station, system_id, started)) => {
                ui.monospace(station).on_hover_text(&session.id);
                ui.monospace(format!("0x{system_id:02x}"));
                ui.monospace(started.format("%Y-%m-%d %H:%M:%S").to_string());
            }
            None => {
                ui.monospace(&session.id);
                ui.label("");
                ui.label("");
            }
        }

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add(Readout {
                value: session.bytes as f32 / (1024.0 * 1024.0),
                decimals: 2,
                width_chars: 6,
                font: egui::TextStyle::Monospace.resolve(ui.style()),
                color: ui.visuals().text_color(),
                ..Default::default()
            });
            ui.monospace(" MiB");
        });

        if session.live() {
            ui.weak("live");
        } else {
            ui.weak(format!("ended {} ago", ago(session.idle)));
        }

        let disabled_hover = if self.own.contains(&session.id) {
            Some("Shared from here")
        } else if self.joined.contains(&session.id) {
            Some("Already joined")
        } else {
            None
        };

        ui.add_enabled(disabled_hover.is_none(), egui::Button::new("Join"))
            .on_disabled_hover_text(disabled_hover.unwrap_or_default())
            .clicked()
    }

    fn fetch(&mut self, ctx: &egui::Context, server: &str) {
        self.in_flight = true;
        self.last_fetch = Some(Instant::now());

        let url = format!("{}/sessions", server.trim_end_matches('/'));
        let arrived = Arc::clone(&self.arrived);
        let ctx = ctx.clone();

        ehttp::fetch(ehttp::Request::get(url), move |result| {
            let listing = match result {
                Ok(response) if response.ok => Ok(SessionSummary::parse_listing(
                    response.text().unwrap_or_default(),
                )),
                Ok(response) => Err(format!("HTTP {}", response.status)),
                Err(e) => Err(e),
            };

            *arrived.lock().unwrap() = Some(listing);
            ctx.request_repaint();
        });
    }
}
