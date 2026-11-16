//! The Claude memory read model. Field order is the JSON key order.

/// Lines of MEMORY.md Claude loads; later lines are invisible.
pub const MAX_INDEX_LINES: usize = 200;
/// Bytes of MEMORY.md Claude loads (25 KB).
pub const MAX_INDEX_BYTES: usize = 25_600;
/// Documented frontmatter `type` values.
pub const MEMORY_TYPES: [&str; 4] = ["user", "feedback", "project", "reference"];

/// Frontmatter shape found on disk.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Shape {
    Nested,
    Flat,
    #[default]
    None,
}

impl Shape {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Nested => "nested",
            Self::Flat => "flat",
            Self::None => "none",
        }
    }
}

/// A named frontmatter field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Description,
    Type,
    NodeType,
    OriginSessionId,
    Created,
    Updated,
}

impl Field {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Description => "description",
            Self::Type => "type",
            Self::NodeType => "nodeType",
            Self::OriginSessionId => "originSessionId",
            Self::Created => "created",
            Self::Updated => "updated",
        }
    }
}

/// Parsed YAML frontmatter, tolerant of the nested `metadata:` block, the
/// legacy flat form and no frontmatter. `raw` keeps the block text so an
/// unchanged block is written back byte for byte.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    pub raw: String,
    pub shape: Shape,
    /// Other keys, in JS property order.
    pub extra: Vec<(String, String)>,
    pub had_trailing_metadata_ws: bool,
    pub description_quoted: bool,
    /// Named fields in the order the file first sets them.
    pub fields: Vec<(Field, String)>,
}

impl Frontmatter {
    pub fn get(&self, f: Field) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| *k == f)
            .map(|(_, v)| v.as_str())
    }

    /// Set a field; a field already set keeps its position.
    pub fn set(&mut self, f: Field, value: &str) {
        match self.fields.iter_mut().find(|(k, _)| *k == f) {
            Some(slot) => value.clone_into(&mut slot.1),
            None => self.fields.push((f, value.to_owned())),
        }
    }
}

/// Outbound link style in a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Wiki,
    Markdown,
}

impl LinkKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wiki => "wiki",
            Self::Markdown => "markdown",
        }
    }
}

/// One outbound link in a memory body. `line` counts from 1 within the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRef {
    pub kind: LinkKind,
    pub raw_target: String,
    pub target_slug: String,
    pub line: usize,
}

/// One memory file, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryEntry {
    pub slug: String,
    pub file_name: String,
    pub body: String,
    /// UTF-8 bytes of the decoded text.
    pub size: usize,
    pub lines: usize,
    /// ISO time, or empty when the mtime is not after 1970.
    pub mtime: String,
    pub mtime_ms: i64,
    pub body_hash: String,
    pub in_index: bool,
    pub links: Vec<LinkRef>,
    pub frontmatter: Frontmatter,
}

impl MemoryEntry {
    pub fn name(&self) -> Option<&str> {
        self.frontmatter.get(Field::Name)
    }

    pub fn description(&self) -> Option<&str> {
        self.frontmatter.get(Field::Description)
    }

    pub fn kind(&self) -> Option<&str> {
        self.frontmatter.get(Field::Type)
    }
}

/// How a MEMORY.md line points at a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexLink {
    Markdown {
        label: String,
        target: String,
        target_slug: String,
        hook: Option<String>,
    },
    Wiki {
        label: String,
        target: String,
        target_slug: String,
        hook: Option<String>,
    },
    /// A bullet that is not a link.
    Bare,
}

/// One parsed MEMORY.md line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub raw: String,
    pub line_number: usize,
    pub link: IndexLink,
}

impl IndexEntry {
    pub fn target(&self) -> Option<&str> {
        match &self.link {
            IndexLink::Markdown { target, .. } | IndexLink::Wiki { target, .. } => Some(target),
            IndexLink::Bare => None,
        }
    }

    /// The target slug, when it is not empty.
    pub fn target_slug(&self) -> Option<&str> {
        match &self.link {
            IndexLink::Markdown { target_slug, .. } | IndexLink::Wiki { target_slug, .. } => {
                Some(target_slug.as_str()).filter(|s| !s.is_empty())
            }
            IndexLink::Bare => None,
        }
    }
}

/// The MEMORY.md index file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryIndex {
    pub path: String,
    pub raw: String,
    pub bytes: usize,
    pub lines: usize,
    pub entries: Vec<IndexEntry>,
    pub is_monolithic: bool,
    pub over_budget: bool,
    pub below_fold_entries: Vec<IndexEntry>,
}

/// What the index budget gauge measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetKind {
    Index,
    Monolithic,
    Absent,
}

impl BudgetKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Index => "index",
            Self::Monolithic => "monolithic",
            Self::Absent => "absent",
        }
    }
}

/// The index budget gauge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexBudget {
    pub lines: usize,
    pub bytes: usize,
    pub over_budget: bool,
    pub below_fold_count: usize,
    pub kind: BudgetKind,
}

/// One graph node: a memory file and its link degrees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNode {
    pub slug: String,
    pub kind: String,
    pub bytes: usize,
    pub in_index: bool,
    pub in_deg: usize,
    pub out_deg: usize,
}

/// Edge source: a body link or a MEMORY.md line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Wiki,
    Markdown,
    Index,
}

impl EdgeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wiki => "wiki",
            Self::Markdown => "markdown",
            Self::Index => "index",
        }
    }
}

impl From<LinkKind> for EdgeKind {
    fn from(k: LinkKind) -> Self {
        match k {
            LinkKind::Wiki => Self::Wiki,
            LinkKind::Markdown => Self::Markdown,
        }
    }
}

/// A link resolved against the vault's files. Unresolved edges carry
/// near-match `candidates`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkEdge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub resolved_to: Option<String>,
    pub ambiguous: bool,
    pub candidates: Option<Vec<String>>,
    pub line: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<LinkEdge>,
    pub orphans: Vec<String>,
}

/// An index line whose target file is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DanglingEntry {
    pub target: String,
    pub line: usize,
}

/// Files the index misses, and index lines with no file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VaultDiff {
    pub orphans: Vec<String>,
    pub dangling: Vec<DanglingEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultState {
    Ok,
    Absent,
    Empty,
    Monolithic,
}

impl VaultState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Absent => "absent",
            Self::Empty => "empty",
            Self::Monolithic => "monolithic",
        }
    }
}

/// One project's whole memory dir, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Entries per frontmatter `type` (`unknown` when missing), JS key order.
    pub type_counts: Vec<(String, usize)>,
    pub total_bytes: usize,
}

/// A project whose memory dir holds files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSummary {
    pub slug: String,
    pub project: String,
    pub memory_dir: String,
    pub file_count: usize,
    pub has_index: bool,
}

/// A body link left pointing at a deleted file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DanglingRef {
    pub from: String,
    pub target: String,
    pub line: usize,
}

/// What a delete left broken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteResult {
    pub slug: String,
    pub dangling: Vec<DanglingRef>,
}
