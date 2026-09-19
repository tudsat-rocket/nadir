use std::sync::{Arc, OnceLock};

use eframe::egui;
use egui::{Align, Layout, RichText};

use crate::build;
use crate::colors::{COLOR_INDICATOR_WARNING, readable};
use crate::widgets::TEXT_SIZE;

const HOVER: &str = "This build is not the current tip of main";
const MAIN_URL: &str = "https://api.github.com/repos/tudsat-rocket/nadir/commits/main";

/// Whether the commit we built from is the tip of main
pub struct Version(Arc<OnceLock<bool>>);

impl Version {
    pub fn check(ctx: &egui::Context) -> Self {
        let up_to_date = Arc::new(OnceLock::new());

        let mut request = ehttp::Request::get(MAIN_URL);
        request
            .headers
            .insert("Accept", "application/vnd.github.sha");
        request.headers.insert("User-Agent", "nadir");

        let slot = Arc::clone(&up_to_date);
        let ctx = ctx.clone();
        ehttp::fetch(request, move |result| {
            let Ok(response) = result else { return };
            let Some(sha) = response.text().filter(|_| response.ok) else {
                return;
            };

            let _ = slot.set(sha.trim().eq_ignore_ascii_case(build::COMMIT_HASH));
            ctx.request_repaint();
        });

        Self(up_to_date)
    }

    pub fn ui(&self, ui: &mut egui::Ui, collapsed: bool) {
        let warning = (self.0.get() == Some(&false)).then(|| {
            RichText::new(if collapsed {
                "⬆"
            } else {
                "⬆ Update available"
            })
            .color(readable(COLOR_INDICATOR_WARNING, ui.visuals()))
            .size(TEXT_SIZE)
        });

        // Collapsed there is no commit to sit beside, so the arrow centres in the strip instead.
        if collapsed {
            if let Some(warning) = warning {
                ui.add_space(6.0);
                ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                    ui.label(warning).on_hover_text(HOVER);
                });
            }
            return;
        }

        ui.horizontal(|ui| {
            let mut commit = build::SHORT_COMMIT.to_owned();
            if !build::GIT_CLEAN {
                commit.push_str("-dirty");
            }
            ui.weak(RichText::new(commit).monospace().size(TEXT_SIZE));

            if let Some(warning) = warning {
                ui.label(warning).on_hover_text(HOVER);
            }
        });
    }
}
