//! Typed read-side shapes of the stats routes, as the core stats writers
//! print them. Only the fields the app reads are typed; the rest
//! (`context`, `fanout`, `web`, `cost`, `paths`) are skipped.

use serde::{Deserialize, Serialize};

use crate::{AgentId, DrillTable};

/// A count split by who made the call: the main thread or a subagent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Split {
    pub main: f64,
    pub nested: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowView {
    pub days: f64,
    pub from_iso: String,
    pub to_iso: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusView {
    pub transcripts: f64,
    pub sessions: f64,
    pub projects: f64,
    pub tool_calls: f64,
    pub bash_calls: f64,
    pub bash_sec: f64,
}

/// One call a row points back to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleRefView {
    pub sid: String,
    pub agent: AgentId,
    pub project: String,
    /// `main`, `subagent` or `workflow-agent`.
    pub kind: String,
    #[serde(default)]
    pub parent: Option<String>,
    pub seq: f64,
    #[serde(default)]
    pub tool_use_id: Option<String>,
    #[serde(default)]
    pub ts: Option<String>,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetailView {
    pub value: String,
    pub hits: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffView {
    pub from: String,
    pub to: String,
    pub pairs: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleMatchView {
    pub path: String,
    pub line: f64,
    pub excerpt: String,
}

/// A memory file that already states a cause's rule, and how it fares.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleCheckView {
    pub matches: Vec<RuleMatchView>,
    #[serde(default)]
    pub since_iso: Option<String>,
    pub hits_since: Split,
    pub calls_since: Split,
    pub per1k_since: Split,
    pub advice: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairSampleView {
    pub failed: SampleRefView,
    pub retry: SampleRefView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CauseRowView {
    pub key: String,
    pub cause: String,
    /// `rule`, `env`, `workflow` or `none`.
    pub kind: String,
    pub label: String,
    #[serde(default)]
    pub fix: Option<String>,
    pub failing: bool,
    pub hits: f64,
    pub flagged: f64,
    pub hits_by: Split,
    pub pairs: f64,
    pub pairs_by: Split,
    pub retry_sessions: f64,
    pub sessions: f64,
    pub projects: f64,
    pub retry_sec: f64,
    pub sec_measured: f64,
    pub retry_ctx_tokens: f64,
    pub tokens_measured: f64,
    pub per1k: Split,
    pub details: Vec<DetailView>,
    pub diffs: Vec<DiffView>,
    #[serde(default)]
    pub rule: Option<RuleCheckView>,
    pub samples: Vec<SampleRefView>,
    pub pair_samples: Vec<PairSampleView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FailRowView {
    pub key: String,
    pub label: String,
    #[serde(rename = "where")]
    pub where_: String,
    pub rows: f64,
    pub sessions: f64,
    pub projects: f64,
    pub flagged: f64,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub detector: Option<String>,
    pub samples: Vec<SampleRefView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeRowView {
    pub key: String,
    pub label: String,
    pub runs: f64,
    pub co_present_runs: f64,
    pub total_sec: f64,
    pub share: f64,
    pub median_sec: f64,
    pub p95_sec: f64,
    pub capped_runs: f64,
    pub capped_sec: f64,
    pub bg_runs: f64,
    pub bg_sec: f64,
    pub samples: Vec<SampleRefView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitShapeView {
    pub key: String,
    pub calls: f64,
    pub total_sec: f64,
    pub share: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitPanelView {
    pub calls: f64,
    pub total_sec: f64,
    pub share: f64,
    pub sessions: f64,
    pub projects: f64,
    pub median_sec: f64,
    pub p95_sec: f64,
    pub by_shape: Vec<WaitShapeView>,
    pub bg_runs: f64,
    pub bg_sec: f64,
    pub bg_declared_sec: f64,
    pub samples: Vec<SampleRefView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterRowView {
    pub key: String,
    pub label: String,
    pub tool: String,
    pub rows: f64,
    pub sessions: f64,
    pub projects: f64,
    pub total_sec: f64,
    pub samples: Vec<SampleRefView>,
}

/// `GET /api/stats/report`, typed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsReportView {
    pub schema_version: String,
    pub generated_at: f64,
    pub window: WindowView,
    pub detector_version: f64,
    pub corpus: CorpusView,
    pub causes: Vec<CauseRowView>,
    pub fails: Vec<FailRowView>,
    pub time: Vec<TimeRowView>,
    pub waiting: WaitPanelView,
    pub clusters: Vec<ClusterRowView>,
    pub skipped: Vec<String>,
    pub caveats: Vec<String>,
}

/// One call behind a drilled row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrillHitView {
    #[serde(flatten)]
    pub sample: SampleRefView,
    #[serde(default)]
    pub dur_sec: Option<f64>,
    pub tool: String,
    pub failed: bool,
    #[serde(default)]
    pub pair_tool_use_id: Option<String>,
    #[serde(default)]
    pub pair_line: Option<String>,
}

/// `GET /api/stats/drill/{table}/{key}`, typed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrillView {
    pub schema_version: String,
    pub table: DrillTable,
    pub key: String,
    pub total: f64,
    pub offset: f64,
    pub hits: Vec<DrillHitView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRowView {
    pub key: String,
    pub turns: f64,
    pub usd: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub total_tokens: f64,
    pub models: Vec<String>,
}

/// `GET /api/stats/usage`, typed (rows and totals only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageView {
    pub schema_version: String,
    pub generated_at: f64,
    pub by: String,
    pub tz: String,
    pub rows: Vec<UsageRowView>,
    pub caveats: Vec<String>,
}

/// `POST /api/stats/refresh`, typed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefreshView {
    pub transcripts: f64,
    pub extracted: f64,
    pub cached: f64,
    pub skipped: Vec<String>,
    pub facts: f64,
}

/// One marked call of `GET /api/sessions/{id}/markers`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerView {
    pub seq: f64,
    #[serde(default)]
    pub tool_use_id: Option<String>,
    pub detector: String,
    pub label: String,
}

/// `GET /api/sessions/{id}/markers`, typed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkersView {
    pub sid: String,
    pub markers: Vec<MarkerView>,
}
