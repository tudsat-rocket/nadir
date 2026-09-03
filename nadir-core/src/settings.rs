use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::LinkId;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub autoconnect_usb: bool,
    pub links: Vec<LinkId>,
    pub map: MapSettings,
    pub theme: Theme,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            autoconnect_usb: true,
            links: Self::default_links(),
            map: MapSettings::default(),
            theme: Theme::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Follows the desktop or browser theme as it changes.
    #[default]
    System,
    Dark,
    Light,
    /// The light theme, retuned so every colour the UI paints clears WCAG 2.2 level AA against the
    /// surface behind it, and so every control has a visible border and focus ring. See
    /// `docs/accessibility-review.md`.
    HighContrast,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MapSettings {
    /// Enables the satellite layer, which needs a Mapbox account.
    pub mapbox_access_token: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("Could not determine a configuration directory")]
    NoConfigDir,
    #[error("Failed to write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Failed to serialize the settings: {0}")]
    Serialize(#[from] toml::ser::Error),
}

impl Settings {
    /// `None` on wasm, where there is no filesystem to keep a file in, so a browser build only ever
    /// has the defaults.
    #[cfg(target_arch = "wasm32")]
    pub fn config_file() -> Option<PathBuf> {
        None
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn config_file() -> Option<PathBuf> {
        let Some(dirs) = directories::ProjectDirs::from("space", "tudsat", "nadir") else {
            tracing::error!(
                "Could not determine a configuration directory, using default settings"
            );
            return None;
        };

        Some(dirs.config_dir().to_path_buf().join("config.toml"))
    }

    /// The default telemetry endpoints, as documented in the README. TODO: remove TCP links?
    fn default_links() -> Vec<LinkId> {
        vec![
            LinkId::UdpServer(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 14550)),
            LinkId::TcpClient(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5760)),
            LinkId::TcpClient(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5761)),
            LinkId::TcpClient(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5762)),
        ]
    }

    /// Reads the settings, falling back to the defaults for anything that is not there.
    pub fn load() -> Self {
        let Some(path) = Self::config_file() else {
            return Self::default();
        };

        Self::load_from(&path)
    }

    /// Writes the settings, creating the directory if it is not there yet.
    pub fn save(&self) -> Result<PathBuf, SaveError> {
        let path = Self::config_file().ok_or(SaveError::NoConfigDir)?;
        self.save_to(&path)?;

        Ok(path)
    }

    fn load_from(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // First run. Write the defaults out so there is something to edit.
                let settings = Self::default();
                match settings.save_to(path) {
                    Ok(()) => tracing::info!("Wrote default settings to {}", path.display()),
                    Err(e) => tracing::error!("{e}"),
                }

                return settings;
            }
            Err(e) => {
                tracing::error!("Failed to read {}: {e}, using defaults", path.display());
                return Self::default();
            }
        };

        match toml::from_str(&text) {
            Ok(settings) => settings,
            Err(e) => {
                tracing::error!("Failed to parse {}: {e}. Using defaults.", path.display());
                Self::default()
            }
        }
    }

    fn save_to(&self, path: &Path) -> Result<(), SaveError> {
        let text = toml::to_string_pretty(self)?;

        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|source| SaveError::Write {
                path: path.to_path_buf(),
                source,
            })?;
        }

        std::fs::write(path, text).map_err(|source| SaveError::Write {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &tempfile::TempDir, text: &str) -> PathBuf {
        let path = dir.path().join("config.toml");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn the_defaults_survive_a_round_trip() {
        let settings = Settings::default();
        let text = toml::to_string_pretty(&settings).unwrap();
        let read: Settings = toml::from_str(&text).unwrap();

        assert_eq!(read.links, settings.links);
        assert_eq!(read.autoconnect_usb, settings.autoconnect_usb);
    }

    /// The layout is a user-facing interface, so pin it rather than only asserting it round-trips.
    #[test]
    fn a_link_is_a_type_and_an_address() {
        let settings = Settings {
            links: vec![LinkId::UdpServer(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                14550,
            ))],
            ..Settings::default()
        };

        let text = toml::to_string_pretty(&settings).unwrap();

        assert!(text.contains("type = \"udp_server\""), "{text}");
        assert!(text.contains("addr = \"0.0.0.0:14550\""), "{text}");
    }

    #[test]
    fn a_theme_is_a_lowercase_name() {
        let settings = Settings {
            theme: Theme::Light,
            ..Settings::default()
        };

        let text = toml::to_string_pretty(&settings).unwrap();
        assert!(text.contains("theme = \"light\""), "{text}");

        let read: Settings = toml::from_str(&text).unwrap();
        assert_eq!(read.theme, Theme::Light);
    }

    /// Multi-word theme names are `snake_case` in the file, not `HighContrast`.
    #[test]
    fn high_contrast_round_trips_as_snake_case() {
        let settings = Settings {
            theme: Theme::HighContrast,
            ..Settings::default()
        };

        let text = toml::to_string_pretty(&settings).unwrap();
        assert!(text.contains("theme = \"high_contrast\""), "{text}");

        let read: Settings = toml::from_str(&text).unwrap();
        assert_eq!(read.theme, Theme::HighContrast);
    }

    #[test]
    fn a_missing_file_is_written_with_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let settings = Settings::load_from(&path);

        assert_eq!(settings.links, Settings::default_links());
        assert!(path.exists(), "the defaults were not written out");
    }

    /// A file that sets one thing must not silently discard everything it does not mention.
    #[test]
    fn a_partial_file_keeps_the_defaults_for_what_it_omits() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "[map]\nmapbox_access_token = \"pk.abc\"\n");

        let settings = Settings::load_from(&path);

        assert_eq!(settings.map.mapbox_access_token, Some("pk.abc".to_owned()));
        assert_eq!(settings.links, Settings::default_links());
        assert!(settings.autoconnect_usb);
    }

    #[test]
    fn a_file_from_a_newer_build_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "autoconnect_usb = false\nsomething_new = 42\n");

        let settings = Settings::load_from(&path);

        assert!(!settings.autoconnect_usb);
        assert_eq!(settings.links, Settings::default_links());
    }

    #[test]
    fn a_broken_file_falls_back_without_being_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let original = "autoconnect_usb = \"not a bool\"\n";
        let path = write(&dir, original);

        let settings = Settings::load_from(&path);

        assert!(settings.autoconnect_usb, "the defaults were not used");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original,
            "someone's hand-edited file was overwritten"
        );
    }

    #[test]
    fn saving_creates_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");

        Settings::default().save_to(&path).unwrap();

        assert!(path.exists());
    }
}
