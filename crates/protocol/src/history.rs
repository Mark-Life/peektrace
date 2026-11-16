//! Shapes of the paged session routes: `/summary`, `/growth`,
//! `/history`, `/history/index` and `/history/search` under
//! `/api/sessions/{id}`. Event-shaped parts are typed in [`crate::view`].

use serde::{Deserialize, Deserializer, Serialize};

use crate::AnalysisQuery;

/// A JSON number, or `null` (`JSON.stringify` of `NaN`) as `NaN`.
fn num<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Option::<f64>::deserialize(d).map(|n| n.unwrap_or(f64::NAN))
}

fn opt_num<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    Option::<f64>::deserialize(d).map(|n| Some(n.unwrap_or(f64::NAN)))
}

fn nums<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<f64>, D::Error> {
    Vec::<Option<f64>>::deserialize(d)
        .map(|v| v.into_iter().map(|n| n.unwrap_or(f64::NAN)).collect())
}

/// `history` of the summary: the parts' hashes. Only equality matters; a
/// client refetches a part whose hash changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SummaryHistory {
    /// Hash of the whole analysis.
    pub rev: String,
    /// Event count.
    pub total: u64,
    /// Events per page in `pages`.
    pub page_size: u64,
    /// Hash per page of `page_size` events.
    pub pages: Vec<String>,
    pub index: String,
    pub growth: String,
}

/// Query of `GET /api/sessions/{id}/history`: the analysis params plus the
/// window. `offset` defaults to 0, `limit` to 100 (at most 500).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HistoryPageQuery {
    #[serde(flatten)]
    pub analysis: AnalysisQuery,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub offset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<String>,
}

/// Query of `GET /api/sessions/{id}/history/search`; `q` is not trimmed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HistorySearchQuery {
    #[serde(flatten)]
    pub analysis: AnalysisQuery,
    pub q: String,
}

/// `GET /api/sessions/{id}/history` without its `events`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HistoryPageMeta {
    pub rev: String,
    pub offset: u64,
    pub total: u64,
    /// Hash of this page's `events`.
    pub hash: String,
}

/// `GET /api/sessions/{id}/history/index` without its `rows`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HistoryIndexMeta {
    pub rev: String,
    pub hash: String,
    pub total: u64,
    /// First event of the turn that crossed into the dumb zone, or -1.
    pub cross_event: i64,
}

/// One turn as the growth chart and chat turn rules read it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct GrowthTurn {
    pub model: String,
    #[serde(deserialize_with = "num")]
    pub context_tokens: f64,
    #[serde(deserialize_with = "num")]
    pub output_tokens: f64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "opt_num"
    )]
    #[cfg_attr(test, ts(optional))]
    pub thinking_tokens: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub effort: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "opt_num"
    )]
    #[cfg_attr(test, ts(optional))]
    pub web_search_requests: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "opt_num"
    )]
    #[cfg_attr(test, ts(optional))]
    pub web_fetch_requests: Option<f64>,
}

/// Tokens per budget category, one column per key, one row per snapshot.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GrowthSlices {
    #[serde(deserialize_with = "nums")]
    pub system_tools: Vec<f64>,
    #[serde(deserialize_with = "nums")]
    pub listings: Vec<f64>,
    #[serde(deserialize_with = "nums")]
    pub memory: Vec<f64>,
    #[serde(deserialize_with = "nums")]
    pub files: Vec<f64>,
    #[serde(deserialize_with = "nums")]
    pub prompts: Vec<f64>,
    #[serde(deserialize_with = "nums")]
    pub tool_results: Vec<f64>,
    #[serde(deserialize_with = "nums")]
    pub assistant_text: Vec<f64>,
    #[serde(deserialize_with = "nums")]
    pub thinking: Vec<f64>,
    #[serde(deserialize_with = "nums")]
    pub other: Vec<f64>,
    #[serde(deserialize_with = "nums")]
    pub unattributed: Vec<f64>,
}

/// The snapshots as columns.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct GrowthSnapshots {
    pub turn_index: Vec<u64>,
    #[serde(deserialize_with = "nums")]
    pub ctx: Vec<f64>,
    #[serde(deserialize_with = "nums")]
    pub output_tokens: Vec<f64>,
    pub slices: GrowthSlices,
}

/// `GET /api/sessions/{id}/growth`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GrowthSeries {
    pub rev: String,
    pub hash: String,
    pub turns: Vec<GrowthTurn>,
    pub snapshots: GrowthSnapshots,
}

/// `GET /api/sessions/{id}/history/search`: ascending event positions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HistorySearchResult {
    pub rev: String,
    pub positions: Vec<u64>,
}
