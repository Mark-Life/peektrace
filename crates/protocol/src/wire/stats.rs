//! The stats report, its drill pages and the per-session markers. Absent
//! values are `null`, never left out.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{AgentId, DrillTable};

/// The report envelope version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum StatsSchemaVersion {
    #[serde(rename = "peektrace-stats/v2")]
    V2,
}

/// Where a transcript sits in the session tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum TranscriptKind {
    Main,
    Subagent,
    WorkflowAgent,
}

/// Intent group a command line is charged to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum Job {
    #[serde(rename = "wait/poll")]
    WaitPoll,
    Test,
    Build,
    Typecheck,
    Lint,
    PackageInstall,
    Net,
    ScriptRun,
    Git,
    Search,
    FileIo,
    Other,
}

/// How a command parks: on the clock, or on a condition it re-checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum WaitShape {
    LoopPoll,
    Watcher,
    CurlRetry,
    BareDelay,
}

/// A named cause of a failed call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum CauseId {
    ZshGlobFlag,
    ZshGlobPath,
    ZshEqualsAbort,
    PkillSelfKill,
    SleepBlocked,
    CommandNotFound,
    OutputTooLarge,
    ReadBeforeWrite,
    StaleRead,
    EditNoMatch,
    StructuredOutputSchema,
    RmGuard,
    ScratchModuleResolution,
    UgrepComplexity,
    Unclassified,
}

/// Where a fix lives: a memory-file line, the host, the workflow prompt, or nowhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum CauseKind {
    Rule,
    Env,
    Workflow,
    None,
}

/// A count split by who made the call; `nested` is subagent plus workflow agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Split {
    pub main: f64,
    pub nested: f64,
}

/// A pointer from an aggregate row back to the call it came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SampleRef {
    pub sid: String,
    pub agent: AgentId,
    pub project: String,
    pub kind: TranscriptKind,
    /// Session a nested transcript runs under; open `sid` through it.
    pub parent: Option<String>,
    /// Index of the fact inside its session; with `sid` a key.
    pub seq: f64,
    pub tool_use_id: Option<String>,
    pub ts: Option<String>,
    /// One redacted, collapsed, clipped line of evidence.
    pub line: String,
}

/// One row of the fail table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FailRow {
    /// The masked signature or the scanner reason. Drill key.
    pub key: String,
    pub label: String,
    #[serde(rename = "where")]
    pub place: String,
    pub rows: f64,
    pub sessions: f64,
    pub projects: f64,
    /// Of `rows`, how many set the agent's error flag.
    pub flagged: f64,
    pub note: Option<String>,
    pub detector: Option<String>,
    pub samples: Vec<SampleRef>,
}

/// One row of the time table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct TimeRow {
    pub key: Job,
    pub label: String,
    pub runs: f64,
    /// Lines holding this job but charged to a higher-priority one.
    pub co_present_runs: f64,
    pub total_sec: f64,
    /// Of `corpus.bashSec`.
    pub share: f64,
    pub median_sec: f64,
    pub p95_sec: f64,
    pub capped_runs: f64,
    pub capped_sec: f64,
    pub bg_runs: f64,
    pub bg_sec: f64,
    pub samples: Vec<SampleRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WaitShapeRow {
    pub key: WaitShape,
    pub calls: f64,
    pub total_sec: f64,
    pub share: f64,
}

/// The waiting panel: the same rows as the `wait/poll` time row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WaitPanel {
    pub calls: f64,
    pub total_sec: f64,
    pub share: f64,
    pub sessions: f64,
    pub projects: f64,
    pub median_sec: f64,
    pub p95_sec: f64,
    pub by_shape: Vec<WaitShapeRow>,
    pub bg_runs: f64,
    pub bg_sec: f64,
    /// Seconds the backgrounded rows declared; never summed in.
    pub bg_declared_sec: f64,
    pub samples: Vec<SampleRef>,
}

/// Errors sharing a masked signature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ClusterRow {
    pub key: String,
    pub label: String,
    pub tool: String,
    pub rows: f64,
    pub sessions: f64,
    pub projects: f64,
    pub total_sec: f64,
    pub samples: Vec<SampleRef>,
}

/// A session ranked by the context it built up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ContextRow {
    pub key: String,
    pub sid: String,
    pub agent: AgentId,
    pub project: String,
    pub kind: TranscriptKind,
    pub max_context_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub output_tokens: f64,
}

/// One workflow run, costed by the prefix every spawned agent pays again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct FanoutRow {
    pub key: String,
    pub project: String,
    pub agents: f64,
    pub prefix_tokens: f64,
    pub tool_calls: f64,
    pub total_sec: f64,
}

/// The same url or query fetched more than once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WebRepeatRow {
    pub key: String,
    pub tool: String,
    pub calls: f64,
    pub sessions: f64,
    pub chars: f64,
    pub same_run: bool,
}

/// Token cost rolled up by one dimension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CostRow {
    pub key: String,
    pub label: String,
    pub sessions: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
}

/// A memory-file line that already states a cause's rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RuleMatch {
    pub path: String,
    pub line: f64,
    pub excerpt: String,
}

/// A rule present in memory, and whether the cause still fires since.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RuleCheck {
    pub matches: Vec<RuleMatch>,
    /// When the rule appeared; null when unknown.
    pub since_iso: Option<String>,
    pub hits_since: Split,
    pub calls_since: Split,
    pub per1k_since: Split,
    pub advice: String,
}

/// The token change a retry made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CauseDiff {
    pub from: String,
    pub to: String,
    pub pairs: f64,
}

/// One culprit value and how often it was named.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CauseDetail {
    pub value: String,
    pub hits: f64,
}

/// A failed call and the similar call that succeeded right after it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PairSample {
    pub failed: SampleRef,
    pub retry: SampleRef,
}

/// One cause: what fired, how often a retry corrected it, and the fix.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CauseRow {
    /// `cause:<id>`, or `cause:unclassified:<tool>`. Drill key.
    pub key: String,
    pub cause: CauseId,
    pub kind: CauseKind,
    pub label: String,
    /// Null only when `kind` is `none`.
    pub fix: Option<String>,
    /// False for waste rather than failure (output too large).
    pub failing: bool,
    pub hits: f64,
    /// Hits a fix would still save when some culprits are already fixed
    /// (command not found: binaries installed since); absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub live_hits: Option<f64>,
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
    pub details: Vec<CauseDetail>,
    pub diffs: Vec<CauseDiff>,
    pub rule: Option<RuleCheck>,
    pub samples: Vec<SampleRef>,
    pub pair_samples: Vec<PairSample>,
}

/// Corpus totals; every share in the report divides by one of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CorpusSummary {
    pub transcripts: f64,
    pub sessions: f64,
    pub projects: f64,
    pub tool_calls: f64,
    pub bash_calls: f64,
    pub bash_sec: f64,
    pub tool_sec: f64,
    pub human_wait_sec: f64,
    #[cfg_attr(test, ts(type = "Record<string, number>"))]
    pub kinds: BTreeMap<String, f64>,
    #[cfg_attr(test, ts(type = "Record<string, number>"))]
    pub agents: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct StatsWindow {
    pub days: f64,
    pub from_iso: String,
    pub to_iso: String,
}

/// `GET /api/stats/report`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct StatsReport {
    pub schema_version: StatsSchemaVersion,
    /// Epoch ms of the report clock.
    pub generated_at: f64,
    pub window: StatsWindow,
    pub detector_version: f64,
    pub corpus: CorpusSummary,
    pub causes: Vec<CauseRow>,
    pub fails: Vec<FailRow>,
    pub time: Vec<TimeRow>,
    pub waiting: WaitPanel,
    pub clusters: Vec<ClusterRow>,
    pub context: Vec<ContextRow>,
    pub fanout: Vec<FanoutRow>,
    pub web: Vec<WebRepeatRow>,
    pub cost: Vec<CostRow>,
    pub skipped: Vec<String>,
    /// Transcript path of every session a sample names, by `sid`.
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    pub paths: BTreeMap<String, String>,
    pub caveats: Vec<String>,
    /// Every sentence the screen shows about this report, written by core.
    pub text: crate::stats_text::StatsText,
}

/// One call behind an aggregate row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct DrillHit {
    #[serde(flatten)]
    pub sample: SampleRef,
    pub dur_sec: Option<f64>,
    pub tool: String,
    pub failed: bool,
    /// For a cause drill: the tool-use id of the retry that corrected this call.
    pub pair_tool_use_id: Option<String>,
    /// That retry's own line; null when no retry corrected this call.
    pub pair_line: Option<String>,
}

/// `GET /api/stats/drill/{table}/{key}`: one page of the rows behind a key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct DrillResult {
    pub schema_version: StatsSchemaVersion,
    pub table: DrillTable,
    pub key: String,
    /// Matching rows before the page cut.
    pub total: f64,
    pub offset: f64,
    pub hits: Vec<DrillHit>,
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    pub paths: BTreeMap<String, String>,
}

/// One marker on a tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SessionMarker {
    pub seq: f64,
    pub tool_use_id: Option<String>,
    pub detector: String,
    pub label: String,
}

/// `GET /api/sessions/{id}/markers`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SessionMarkers {
    pub schema_version: StatsSchemaVersion,
    pub sid: String,
    pub detector_version: f64,
    pub markers: Vec<SessionMarker>,
}
