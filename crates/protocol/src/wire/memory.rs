//! The Claude memory read model and the answers of its writes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Frontmatter layout seen on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum SchemaShape {
    Nested,
    Flat,
    None,
}

/// Parsed YAML frontmatter; `raw` keeps the block text so a write can
/// re-emit it unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct Frontmatter {
    pub raw: String,
    pub shape: SchemaShape,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    pub extra: BTreeMap<String, String>,
    pub had_trailing_metadata_ws: bool,
    pub description_quoted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum LinkKind {
    Wiki,
    Markdown,
}

/// One outbound link in a memory body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LinkRef {
    pub kind: LinkKind,
    pub raw_target: String,
    pub target_slug: String,
    pub line: f64,
}

/// One memory topic file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    pub slug: String,
    pub file_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub body: String,
    pub size: f64,
    pub lines: f64,
    /// mtime as ISO text.
    pub mtime: String,
    /// mtime in epoch ms, for compare-and-swap.
    pub mtime_ms: f64,
    pub body_hash: String,
    pub in_index: bool,
    pub links: Vec<LinkRef>,
    pub frontmatter: Frontmatter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum IndexEntryKind {
    Markdown,
    Wiki,
    Bare,
}

/// One parsed line of `MEMORY.md`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    pub raw: String,
    pub line_number: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook: Option<String>,
    pub malformed: bool,
    pub kind: IndexEntryKind,
}

/// The `MEMORY.md` index file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MemoryIndex {
    pub exists: bool,
    pub path: String,
    pub raw: String,
    pub bytes: f64,
    pub lines: f64,
    pub entries: Vec<IndexEntry>,
    pub is_monolithic: bool,
    pub over_budget: bool,
    pub below_fold_entries: Vec<IndexEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum BudgetKind {
    Index,
    Monolithic,
    Absent,
}

/// The index budget: the headline gauge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct IndexBudget {
    pub lines: f64,
    pub max_lines: f64,
    pub bytes: f64,
    pub max_bytes: f64,
    pub over_budget: bool,
    pub below_fold_count: f64,
    pub kind: BudgetKind,
}

/// One node of the link graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub slug: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub bytes: f64,
    pub in_index: bool,
    pub in_deg: f64,
    pub out_deg: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum LinkEdgeKind {
    Wiki,
    Markdown,
    Index,
}

/// A resolved edge: a body link or an index entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct LinkEdge {
    pub from: String,
    pub to: String,
    pub kind: LinkEdgeKind,
    pub resolved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ambiguous: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<LinkEdge>,
    pub orphans: Vec<String>,
}

/// An index entry whose target file is missing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DanglingEntry {
    pub target: String,
    pub line: f64,
}

/// Files not in the index, and index entries with no file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VaultDiff {
    pub orphans: Vec<String>,
    pub dangling: Vec<DanglingEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum VaultState {
    Ok,
    Absent,
    Empty,
    Monolithic,
}

/// One project's whole parsed vault.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MemoryVault {
    pub slug: String,
    pub project: String,
    pub memory_dir: String,
    pub state: VaultState,
    pub index: Option<MemoryIndex>,
    pub entries: Vec<MemoryEntry>,
    pub stray_files: Vec<String>,
    pub budget: IndexBudget,
    pub graph: GraphData,
    pub diff: VaultDiff,
    #[cfg_attr(test, ts(type = "Record<string, number>"))]
    pub type_counts: BTreeMap<String, f64>,
    pub total_bytes: f64,
}

/// A project with a non-empty memory dir.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub slug: String,
    pub project: String,
    pub memory_dir: String,
    pub file_count: f64,
    pub has_index: bool,
}

/// A Claude project dir and its display label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProjectLabel {
    pub slug: String,
    pub project: String,
}

/// `GET /api/memory`: every vault, plus the label of every Claude project dir.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AllVaults {
    pub projects: Vec<ProjectSummary>,
    pub vaults: Vec<MemoryVault>,
    pub labels: Vec<ProjectLabel>,
}

/// A body reference a delete leaves dangling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
pub struct DanglingRef {
    pub from: String,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<f64>,
}

/// The answer of a delete: the references it leaves broken.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DeleteResult {
    pub slug: String,
    pub dangling: Vec<DanglingRef>,
}
