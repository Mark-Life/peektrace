//! Typed read-side shapes of the memory routes, as the core JSON writers
//! print them. Only the fields the app reads are typed.

use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// `GET /api/memory`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AllVaultsView {
    pub projects: Vec<ProjectSummaryView>,
    pub vaults: Vec<VaultView>,
    pub labels: Vec<ProjectLabelView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummaryView {
    pub slug: String,
    pub project: String,
    pub file_count: u64,
    pub has_index: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectLabelView {
    pub slug: String,
    pub project: String,
}

/// One project's vault.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultView {
    pub slug: String,
    pub project: String,
    pub entries: Vec<EntryView>,
    pub budget: BudgetView,
    pub graph: GraphView,
    pub diff: DiffView,
    pub type_counts: TypeCounts,
    pub total_bytes: u64,
}

/// One memory file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    pub slug: String,
    pub file_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub body: String,
    pub size: u64,
    pub lines: u64,
    pub mtime: String,
    pub mtime_ms: f64,
    pub in_index: bool,
    pub links: Vec<LinkView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkView {
    pub kind: String,
    pub raw_target: String,
    pub target_slug: String,
    pub line: u64,
}

/// The MEMORY.md budget; `kind` is `index`, `monolithic` or `absent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetView {
    pub lines: u64,
    pub max_lines: u64,
    pub bytes: u64,
    pub max_bytes: u64,
    pub over_budget: bool,
    pub below_fold_count: u64,
    pub kind: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphView {
    pub nodes: Vec<GraphNodeView>,
    pub edges: Vec<GraphEdgeView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeView {
    pub slug: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub bytes: u64,
    pub in_index: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdgeView {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub resolved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffView {
    pub orphans: Vec<String>,
    pub dangling: Vec<DanglingView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DanglingView {
    pub target: String,
    pub line: u64,
}

/// The answer to a delete: references now broken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteView {
    pub slug: String,
    pub dangling: Vec<DanglingRefView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DanglingRefView {
    pub from: String,
    pub target: String,
    pub line: u64,
}

/// `typeCounts` in document order: the core writer already prints JS key
/// order, and a map type would lose it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeCounts(pub Vec<(String, u64)>);

impl<'de> Deserialize<'de> for TypeCounts {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Ordered;
        impl<'de> Visitor<'de> for Ordered {
            type Value = TypeCounts;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object of counts")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<TypeCounts, A::Error> {
                let mut out = Vec::with_capacity(map.size_hint().unwrap_or(0));
                while let Some((k, v)) = map.next_entry::<String, u64>()? {
                    out.push((k, v));
                }
                Ok(TypeCounts(out))
            }
        }
        d.deserialize_map(Ordered)
    }
}

impl Serialize for TypeCounts {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(self.0.iter().map(|(k, v)| (k, v)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_counts_keep_document_order() {
        let t: TypeCounts =
            serde_json::from_str(r#"{"42":1,"unknown":3,"user":4,"feedback":0}"#).unwrap();
        let keys: Vec<&str> = t.0.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["42", "unknown", "user", "feedback"]);
        assert_eq!(t.0[2].1, 4);
        assert_eq!(
            serde_json::to_string(&t).unwrap(),
            r#"{"42":1,"unknown":3,"user":4,"feedback":0}"#
        );
    }

    #[test]
    fn entry_reads_optional_frontmatter() {
        let e: EntryView = serde_json::from_str(
            r#"{"slug":"a","fileName":"a.md","body":"x","size":1,"lines":1,"mtime":"m","mtimeMs":1.5,"bodyHash":"h","inIndex":false,"links":[],"frontmatter":{}}"#,
        )
        .unwrap();
        assert_eq!(e.kind, None);
        assert!((e.mtime_ms - 1.5).abs() < f64::EPSILON);
    }
}
