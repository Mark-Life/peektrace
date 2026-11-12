//! Request and response shapes of the peektrace HTTP API under `/api`.
//!
//! Session headers, parsed sessions and analyses are written by the core
//! JSON writers, so only their names appear here.

#[cfg(test)]
mod ts_gen;

use serde::{Deserialize, Serialize};

/// A coding agent id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum AgentId {
    Claude,
    Codex,
    Pi,
    Opencode,
}

impl AgentId {
    pub const ALL: [Self; 4] = [Self::Claude, Self::Codex, Self::Pi, Self::Opencode];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Pi => "pi",
            Self::Opencode => "opencode",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.as_str() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum SupportLevel {
    Supported,
    Partial,
    Planned,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CapabilitySupport {
    pub level: SupportLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PerAgentSupport {
    pub claude: CapabilitySupport,
    pub codex: CapabilitySupport,
    pub pi: CapabilitySupport,
    pub opencode: CapabilitySupport,
}

/// One row of the feature x agent matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    pub id: String,
    pub group: String,
    pub title: String,
    pub description: String,
    pub per_agent: PerAgentSupport,
}

/// One configured root of an agent and whether it exists now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScannedRoot {
    pub is_default: bool,
    pub home: String,
    pub path: String,
    pub exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AgentScan {
    pub agent: AgentId,
    pub roots: Vec<ScannedRoot>,
}

/// `GET /api/server`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub read_only: bool,
    pub user_home: String,
    pub agents: Vec<AgentScan>,
}

/// One extra root in `settings.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RootEntry {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub label: Option<String>,
}

/// The `settings.json` document. `roots` keys are agent ids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PeektraceSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "Partial<Record<AgentId, readonly RootEntry[]>>"))]
    pub roots: Option<serde_json::Map<String, serde_json::Value>>,
}

/// `GET /api/settings` and the `PUT` answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SettingsResult {
    pub settings: PeektraceSettings,
    #[cfg_attr(test, ts(type = "number"))]
    pub mtime_ms: i64,
    pub path: String,
}

/// `PUT /api/settings` body. `expectedMtime` turns on compare-and-swap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdate {
    pub settings: PeektraceSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub expected_mtime: Option<f64>,
}

/// Change counters: a client refetches when one goes up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WatchVersions {
    #[cfg_attr(test, ts(type = "number"))]
    pub memory: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub sessions: u64,
}

/// A transcript the listing could not read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct UnreadableSession {
    pub agent: AgentId,
    pub project: String,
    pub path: String,
}

/// `GET /api/sessions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionListing {
    #[cfg_attr(test, ts(type = "SessionHeader[]"))]
    pub headers: Vec<serde_json::Value>,
    pub unreadable: Vec<UnreadableSession>,
}

/// Query of `GET /api/sessions`. Empty values mean no filter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionsQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "AgentId | \"\""))]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub project: Option<String>,
}

/// Query of `GET /api/sessions/{id}` and the image routes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "\"true\" | \"false\""))]
    pub redact: Option<String>,
}

/// Query of `GET /api/sessions/{id}/analysis`; numbers as decimal text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AnalysisQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub window: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub dumb_zone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "\"true\" | \"false\""))]
    pub redact: Option<String>,
}

/// Why a settings compare-and-swap failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum FileChangeReason {
    Mtime,
    Hash,
    Missing,
}

/// A feature whose routes answer 501 in this build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum NotPortedFeature {
    Stats,
    Memory,
    Search,
}

/// Every failure an `/api` route answers with, as `{"error": ...}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "_tag")]
pub enum ApiError {
    #[serde(rename_all = "camelCase")]
    SessionNotFoundError {
        id: String,
        searched_root: String,
    },
    TranscriptParseError {
        path: String,
        reason: String,
    },
    FileChangedError {
        path: String,
        reason: FileChangeReason,
    },
    SettingsWriteError {
        path: String,
        reason: String,
    },
    ImageNotFoundError {
        id: String,
        #[cfg_attr(test, ts(type = "number"))]
        line: u64,
        #[cfg_attr(test, ts(type = "number"))]
        block: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional, type = "number"))]
        item: Option<u64>,
    },
    BadRequestError {
        reason: String,
    },
    NotFoundError {
        path: String,
    },
    MethodNotAllowedError {
        method: String,
    },
    NotPortedYetError {
        feature: NotPortedFeature,
    },
    PathOutsideRootError {
        path: String,
        roots: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    CapabilityUnsupportedError {
        capability_id: String,
        agent_id: AgentId,
    },
    MemoryValidationError {
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        name: Option<String>,
    },
    MemoryNotFoundError {
        project: String,
        name: String,
    },
    StatsScanError {
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        path: Option<String>,
    },
    StatsRowNotFoundError {
        key: String,
        table: String,
    },
}

impl ApiError {
    /// The HTTP status this error is sent with.
    pub const fn status(&self) -> u16 {
        match self {
            Self::BadRequestError { .. } => 400,
            Self::SettingsWriteError { reason, .. } if is_read_only_reason(reason.as_str()) => 403,
            Self::CapabilityUnsupportedError { .. } | Self::PathOutsideRootError { .. } => 403,
            Self::SessionNotFoundError { .. }
            | Self::ImageNotFoundError { .. }
            | Self::NotFoundError { .. }
            | Self::MemoryNotFoundError { .. }
            | Self::StatsRowNotFoundError { .. } => 404,
            Self::MethodNotAllowedError { .. } => 405,
            Self::FileChangedError { .. } => 409,
            Self::MemoryValidationError { .. } => 422,
            Self::NotPortedYetError { .. } => 501,
            Self::TranscriptParseError { .. }
            | Self::SettingsWriteError { .. }
            | Self::StatsScanError { .. } => 500,
        }
    }
}

/// Reason the server gives when read-only mode refuses a write.
pub const READ_ONLY_REASON: &str = "read-only mode is enabled; refusing to write";

const fn is_read_only_reason(reason: &str) -> bool {
    let (a, b) = (reason.as_bytes(), READ_ONLY_REASON.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// The body of every failed `/api` response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ApiErrorBody {
    pub error: ApiError,
}

impl ApiErrorBody {
    pub fn json(error: ApiError) -> String {
        serde_json::to_string(&Self { error }).unwrap_or_default()
    }
}
