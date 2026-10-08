use std::time::Duration;

use eframe::egui;
use nadir_core::System;

use crate::panes::PaneUi;
use crate::widgets::Hazard;

/// Only one quick disconnect is shown at a time, so operators cannot mix them up mid-operation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Qd {
    Oxidizer,
    Pressurant,
}

impl Qd {
    const ALL: [Self; 2] = [Self::Oxidizer, Self::Pressurant];

    fn label(self) -> &'static str {
        match self {
            Self::Oxidizer => "Oxidizer QD",
            Self::Pressurant => "Pressurant(N2) QD",
        }
    }

    /// The 1-based gripper/winch instance, matching QD 1/2 of the payload pane.
    fn instance(self) -> u8 {
        match self {
            Self::Oxidizer => 2,
            Self::Pressurant => 1,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Oxidizer => 0,
            Self::Pressurant => 1,
        }
    }
}

/// Retracting before the disconnect sequence can be catastrophic, so retraction is gated behind a
/// tickbox: an override before the disconnect command was sent, a visual confirmation after.
#[derive(Default)]
struct QdState {
    disconnect_sent: bool,
    /// When the tickbox was ticked, in `egui` input time.
    ticked_at: Option<f64>,
}

impl QdState {
    /// An override lapses so a forgotten tick cannot enable a retraction much later.
    const OVERRIDE_SECS: f64 = 10.0;

    /// Drops an override that has run out. A visual confirmation does not lapse.
    fn expire(&mut self, now: f64) {
        if !self.disconnect_sent
            && self
                .ticked_at
                .is_some_and(|at| now - at >= Self::OVERRIDE_SECS)
        {
            self.ticked_at = None;
        }
    }

    /// An override only covers a single retraction.
    fn retracted(&mut self) {
        if !self.disconnect_sent {
            self.ticked_at = None;
        }
    }
}

pub struct QdRetractionPane {
    selected: Qd,
    states: [QdState; 2],
}

impl QdRetractionPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {
            selected: Qd::Oxidizer,
            states: Default::default(),
        }
    }

    fn button(ui: &mut egui::Ui, system: &System, label: &str) -> bool {
        let response = ui.button(label);
        Hazard::tick(ui, response.rect, system);
        Hazard::confirm(ui, &response, system)
    }
}

impl PaneUi for QdRetractionPane {
    fn system_ui(&mut self, ui: &mut egui::Ui, system: System) {
        if system.muted() {
            ui.disable();
        }

        let previous = self.selected;
        egui::ComboBox::from_id_salt("qd retraction")
            .selected_text(self.selected.label())
            .show_ui(ui, |ui| {
                for qd in Qd::ALL {
                    ui.selectable_value(&mut self.selected, qd, qd.label());
                }
            });
        if self.selected != previous {
            // Overrides do not carry across a switch.
            self.states[previous.index()].retracted();
        }

        let instance = self.selected.instance();
        let now = ui.input(|i| i.time);
        let state = &mut self.states[self.selected.index()];
        state.expire(now);

        if Self::button(ui, &system, "Start Disconnect Sequence") {
            system.do_gripper(instance, false);
            // A repeated disconnect suggests the last one failed, so it needs looking at again.
            state.disconnect_sent = true;
            state.ticked_at = None;
        }

        let mut ticked = state.ticked_at.is_some();
        let text = if state.disconnect_sent {
            egui::RichText::new("Please confirm visually")
        } else {
            egui::RichText::new("⚠ Override: retract without disconnect")
                .color(ui.visuals().warn_fg_color)
        };
        if ui.checkbox(&mut ticked, text).changed() {
            state.ticked_at = ticked.then_some(now);
        }
        if let Some(at) = state.ticked_at
            && !state.disconnect_sent
        {
            let left = QdState::OVERRIDE_SECS - (now - at);
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(left.max(0.0)));
        }

        ui.add_enabled_ui(state.ticked_at.is_some(), |ui| {
            ui.horizontal(|ui| {
                if Self::button(ui, &system, "Retract 10%") {
                    system.do_winch_relative(instance, 100.0);
                    state.retracted();
                }
                if Self::button(ui, &system, "Retract 100%") {
                    system.do_winch_relative(instance, 1000.0);
                    state.retracted();
                }
            });
        });

        let _ = ui.button("Placeholder");
    }
}
