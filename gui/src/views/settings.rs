//! The Preferences screen: edits the settings file.

use std::path::PathBuf;

use core::{LinkId, Settings};

use eframe::egui;

use crate::widgets::column_header;

/// What kind of endpoint a link is, separated from its address so the two can be edited apart.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    UdpServer,
    TcpClient,
    SerialPort,
}

impl Kind {
    const ALL: [Self; 3] = [Self::UdpServer, Self::TcpClient, Self::SerialPort];

    fn label(self) -> &'static str {
        match self {
            Self::UdpServer => "UDP Server",
            Self::TcpClient => "TCP Client",
            Self::SerialPort => "Serial Port",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::UdpServer | Self::TcpClient => "host:port",
            Self::SerialPort => "/dev/ttyUSB0",
        }
    }

    /// The link this describes, or why it is not one yet.
    fn build(self, addr: &str) -> Result<LinkId, &'static str> {
        match self {
            Self::SerialPort if addr.is_empty() => Err("expected a device path"),
            Self::SerialPort => Ok(LinkId::SerialPort(addr.to_owned())),
            Self::UdpServer | Self::TcpClient => {
                let addr = addr
                    .parse()
                    .map_err(|_e| "expected an address and port, e.g. 127.0.0.1:5760")?;

                Ok(match self {
                    Self::UdpServer => LinkId::UdpServer(addr),
                    _ => LinkId::TcpClient(addr),
                })
            }
        }
    }
}

/// One link as it is being edited. Held as text, because an address half-typed is not yet an address.
struct LinkDraft {
    kind: Kind,
    addr: String,
}

impl From<&LinkId> for LinkDraft {
    fn from(link: &LinkId) -> Self {
        match link {
            LinkId::UdpServer(addr) => Self {
                kind: Kind::UdpServer,
                addr: addr.to_string(),
            },
            LinkId::TcpClient(addr) => Self {
                kind: Kind::TcpClient,
                addr: addr.to_string(),
            },
            LinkId::SerialPort(path) => Self {
                kind: Kind::SerialPort,
                addr: path.clone(),
            },
        }
    }
}

pub struct SettingsView {
    links: Vec<LinkDraft>,
    autoconnect_usb: bool,
    mapbox_access_token: String,
    /// What came of the last save, kept on screen until the next one.
    status: Option<Result<PathBuf, String>>,
}

impl SettingsView {
    pub fn new(settings: &Settings) -> Self {
        Self {
            links: settings.links.iter().map(LinkDraft::from).collect(),
            autoconnect_usb: settings.autoconnect_usb,
            mapbox_access_token: settings.map.mapbox_access_token.clone().unwrap_or_default(),
            status: None,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(10.0);
            ui.indent("settings", |ui| {
                let links = self.links_ui(ui);

                ui.add_space(15.0);
                self.map_ui(ui);

                ui.add_space(15.0);
                self.save_ui(ui, links);
            });
        });
    }

    /// Returns the links as they currently parse, so the save button can refuse a broken one.
    fn links_ui(&mut self, ui: &mut egui::Ui) -> Result<Vec<LinkId>, ()> {
        column_header(ui, "🖧 LINKS");

        let mut remove = None;
        let mut built = Ok(Vec::with_capacity(self.links.len()));

        for (i, link) in self.links.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add_space(5.0);

                egui::ComboBox::from_id_salt(("link kind", i))
                    .selected_text(link.kind.label())
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for kind in Kind::ALL {
                            ui.selectable_value(&mut link.kind, kind, kind.label());
                        }
                    });

                ui.add(
                    egui::TextEdit::singleline(&mut link.addr)
                        .desired_width(200.0)
                        .hint_text(link.kind.hint()),
                );

                match link.kind.build(&link.addr) {
                    Ok(id) => {
                        if let Ok(links) = &mut built {
                            links.push(id);
                        }
                    }
                    Err(why) => {
                        ui.colored_label(crate::colors::COLOR_INDICATOR_LIMITS, "⚠")
                            .on_hover_text(why);
                        built = Err(());
                    }
                }

                if ui.small_button("✖").on_hover_text("Remove").clicked() {
                    remove = Some(i);
                }
            });
        }

        if let Some(i) = remove {
            self.links.remove(i);
        }

        ui.horizontal(|ui| {
            ui.add_space(5.0);
            if ui.button("➕ Add link").clicked() {
                self.links.push(LinkDraft {
                    kind: Kind::UdpServer,
                    addr: String::new(),
                });
            }
        });

        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.add_space(5.0);
            ui.checkbox(
                &mut self.autoconnect_usb,
                "Open USB serial ports as they appear",
            );
        });

        built
    }

    fn map_ui(&mut self, ui: &mut egui::Ui) {
        column_header(ui, "🗺 MAP");

        ui.horizontal(|ui| {
            ui.add_space(5.0);
            ui.label("Mapbox access token");
            ui.add(
                egui::TextEdit::singleline(&mut self.mapbox_access_token)
                    .desired_width(260.0)
                    .password(true)
                    .hint_text("none: satellite view is off"),
            );
        });
    }

    fn save_ui(&mut self, ui: &mut egui::Ui, links: Result<Vec<LinkId>, ()>) {
        ui.horizontal(|ui| {
            ui.add_space(5.0);

            let ok = links.is_ok();
            if ui
                .add_enabled(ok, egui::Button::new("💾 Save"))
                .on_disabled_hover_text("One of the links is not a valid address")
                .clicked()
            {
                self.status = Some(self.save(links.unwrap_or_default()));
            }

            match &self.status {
                Some(Ok(path)) => {
                    ui.weak(format!("Saved to {}", path.display()));
                }
                Some(Err(e)) => {
                    ui.colored_label(crate::colors::COLOR_INDICATOR_LIMITS, e);
                }
                None => {}
            }
        });

        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.add_space(5.0);
            // Honest rather than tidy: there is no way to retire a link once `Core` has spawned it.
            ui.weak("Settings are read at startup. Restart to apply them.");
        });
    }

    fn save(&self, links: Vec<LinkId>) -> Result<PathBuf, String> {
        let settings = Settings {
            autoconnect_usb: self.autoconnect_usb,
            links,
            map: core::settings::MapSettings {
                mapbox_access_token: (!self.mapbox_access_token.is_empty())
                    .then_some(self.mapbox_access_token.clone()),
            },
        };

        settings.save().map_err(|e| e.to_string())
    }
}
