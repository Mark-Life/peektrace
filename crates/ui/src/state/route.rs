//! Where the window is and the loads the app layer runs.

use peektrace_protocol::SettingsUpdate;

use crate::model::stats::{DrillAsk, StatsScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Route {
    Sessions,
    Stats,
    Memory,
    Settings,
    Capabilities,
}

impl Route {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Sessions => "Sessions",
            Self::Stats => "Stats",
            Self::Memory => "Memory",
            Self::Settings => "Settings",
            Self::Capabilities => "Capabilities",
        }
    }
}

/// A load the app layer runs; `rid` names the request in its slot.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Sessions {
        rid: u64,
    },
    Analysis {
        rid: u64,
        id: String,
        parent: Option<String>,
    },
    StatsReport {
        rid: u64,
        scope: StatsScope,
    },
    StatsDrill {
        rid: u64,
        scope: StatsScope,
        ask: DrillAsk,
    },
    Capabilities {
        rid: u64,
    },
    ServerInfo {
        rid: u64,
    },
    Settings {
        rid: u64,
    },
    SaveSettings {
        rid: u64,
        update: SettingsUpdate,
    },
}
