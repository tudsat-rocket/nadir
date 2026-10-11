use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use nadir_core::{SessionAddr, SessionSummary};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

type Listing = Option<Result<Vec<SessionSummary>, String>>;

/// The sessions a `meridian` service lists, fetched while the menu showing them is open.
#[derive(Default)]
pub struct Sessions {
    listing: Arc<Mutex<Listing>>,
    in_flight: Arc<AtomicBool>,
    last_fetch: Option<Instant>,
}

impl Sessions {
    /// The Join menu, returning the session picked.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        collapsed: bool,
        server: Option<&str>,
    ) -> Option<SessionAddr> {
        let mut picked = None;

        let Some(server) = server else {
            ui.add_enabled(
                false,
                egui::Button::new(if collapsed {
                    "📡"
                } else {
                    "📡 Join Session"
                }),
            )
            .on_disabled_hover_text("Set a sharing server in Preferences");
            return None;
        };

        ui.menu_button(
            if collapsed {
                "📡"
            } else {
                "📡 Join Session"
            },
            |ui| {
                self.refresh(ui.ctx(), server);
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

                match &*self.listing.lock().unwrap() {
                    None => {
                        ui.spinner();
                    }
                    Some(Err(e)) => {
                        ui.weak(e);
                    }
                    Some(Ok(sessions)) if sessions.is_empty() => {
                        ui.weak("No sessions");
                    }
                    Some(Ok(sessions)) => {
                        let mut sessions = sessions.clone();
                        sessions.sort_by_key(|s| (!s.live(), s.idle));

                        for session in sessions {
                            let label = format!(
                                "{} {}  {:.1} MiB",
                                if session.live() { "🔴" } else { "⏹" },
                                session.id,
                                session.bytes as f64 / (1024.0 * 1024.0),
                            );

                            if ui.button(label).clicked() {
                                picked = Some(SessionAddr {
                                    server: server.to_owned(),
                                    id: session.id,
                                });
                            }
                        }
                    }
                }
            },
        );

        picked
    }

    fn refresh(&mut self, ctx: &egui::Context, server: &str) {
        ctx.request_repaint_after(REFRESH_INTERVAL);

        if self.in_flight.load(Ordering::Relaxed)
            || self
                .last_fetch
                .is_some_and(|at| at.elapsed() < REFRESH_INTERVAL)
        {
            return;
        }

        self.in_flight.store(true, Ordering::Relaxed);
        self.last_fetch = Some(Instant::now());

        let url = format!("{}/sessions", server.trim_end_matches('/'));
        let listing = Arc::clone(&self.listing);
        let in_flight = Arc::clone(&self.in_flight);
        let ctx = ctx.clone();

        ehttp::fetch(ehttp::Request::get(url), move |result| {
            let parsed = match result {
                Ok(response) if response.ok => Ok(SessionSummary::parse_listing(
                    response.text().unwrap_or_default(),
                )),
                Ok(response) => Err(format!("HTTP {}", response.status)),
                Err(e) => Err(e),
            };

            *listing.lock().unwrap() = Some(parsed);
            in_flight.store(false, Ordering::Relaxed);
            ctx.request_repaint();
        });
    }
}
