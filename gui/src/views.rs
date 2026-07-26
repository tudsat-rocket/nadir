mod overview;
pub use overview::Overview;

/// Identifies one open data source: [`LIVE`] is always the links, the rest are telemetry logs.
pub type SourceId = u32;

pub const LIVE: SourceId = 0;

#[derive(Clone, Copy, PartialEq)]
pub enum View {
    Overview,
    Settings,
    /// A recording carries the same system id as the vehicle it came from, so the source is part of
    /// the identity rather than the system id alone.
    System {
        source: SourceId,
        system_id: u8,
    },
}

impl View {
    pub fn system(source: SourceId, system_id: u8) -> Self {
        Self::System { source, system_id }
    }

    pub fn source(self) -> Option<SourceId> {
        match self {
            Self::System { source, .. } => Some(source),
            Self::Overview | Self::Settings => None,
        }
    }
}
