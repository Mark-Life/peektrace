//! The memory explorer's rules: the write gate, project options, filters,
//! sort, body links and the text the views show. Copies of the inspector's
//! `routes/memory` and `lib/memory-support` helpers; the parity `ui` stage
//! keeps them equal.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Write as _;

use peektrace_js::num::{to_fixed, to_js_string};
use peektrace_js::text::{cmp_utf16, locale_compare};
use peektrace_js::time::{LocalZone, parse_date, to_locale_string};
use peektrace_protocol::view::SessionHeader;
use peektrace_protocol::view::memory::{
    AllVaultsView, DeleteView, EntryView, ProjectLabelView, ProjectSummaryView, VaultView,
};
use peektrace_protocol::{AgentId, ApiError, Capability, CapabilitySupport, SupportLevel};

use super::list::agent_label;

/// The frontmatter types a memory may have.
pub const MEMORY_TYPES: [&str; 4] = ["user", "feedback", "project", "reference"];
/// The filter value that keeps everything.
pub const ALL: &str = "all";
/// The type filter value for entries with no type.
pub const UNTYPED: &str = "untyped";
/// The capability that gates every memory write.
pub const MEMORY_CRUD: &str = "memory.crud";
/// The text under a vault, the table and the graph when nothing is left.
pub const NO_MATCHES: &str = "No memories match the current filters.";
pub const READ_ONLY_TITLE: &str = "Read-only server";
pub const READ_ONLY_BODY: &str = "The server was started with --read-only, or on a non-loopback --host without --allow-write. Memories can't be edited from here.";

/// The type filter: `(value, label)`, every type then `untyped`.
pub fn type_filter_options() -> Vec<(&'static str, &'static str)> {
    std::iter::once((ALL, "All types"))
        .chain(MEMORY_TYPES.iter().map(|t| (*t, *t)))
        .chain(std::iter::once((UNTYPED, UNTYPED)))
        .collect()
}

/// The `memory.crud` cell of each agent, in [`AgentId::ALL`] order; `None`
/// when the row or the cell is missing.
pub type CrudCells = [Option<CapabilitySupport>; 4];

pub fn crud_cells(caps: &[Capability]) -> CrudCells {
    caps.iter()
        .find(|c| c.id == MEMORY_CRUD)
        .map_or([None, None, None, None], |c| {
            let p = &c.per_agent;
            [&p.claude, &p.codex, &p.pi, &p.opencode].map(|s| Some(s.clone()))
        })
}

const fn level_of(cell: Option<&CapabilitySupport>) -> SupportLevel {
    match cell {
        Some(c) => c.level,
        None => SupportLevel::Unsupported,
    }
}

/// `Memory editing: Claude. Planned: Codex, Pi, OpenCode.`
pub fn support_summary(cells: &CrudCells) -> String {
    let names_at = |level: SupportLevel| {
        AgentId::ALL
            .iter()
            .zip(cells)
            .filter(|(_, c)| level_of(c.as_ref()) == level)
            .map(|(a, _)| agent_label(*a))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let supported = names_at(SupportLevel::Supported);
    let others: String = [
        (SupportLevel::Partial, "Partial"),
        (SupportLevel::Planned, "Planned"),
        (SupportLevel::Unsupported, "Not supported"),
    ]
    .into_iter()
    .map(|(level, label)| (label, names_at(level)))
    .filter(|(_, names)| !names.is_empty())
    .fold(String::new(), |mut out, (label, names)| {
        let _ = write!(out, " {label}: {names}.");
        out
    });
    let who = if supported.is_empty() {
        "no agent yet"
    } else {
        supported.as_str()
    };
    format!("Memory editing: {who}.{others}")
}

/// What stops memory writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    ReadOnly,
    Capability,
}

impl Blocked {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::Capability => "capability",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub level: SupportLevel,
    pub note: Option<String>,
    pub blocked_by: Option<Blocked>,
}

impl Verdict {
    pub const fn can_write(&self) -> bool {
        self.blocked_by.is_none()
    }
}

/// A read-only server blocks first; else Claude's cell decides.
pub fn crud_verdict(cells: &CrudCells, read_only: bool) -> Verdict {
    let claude = cells[0].as_ref();
    let level = level_of(claude);
    let blocked_by = if read_only {
        Some(Blocked::ReadOnly)
    } else if level == SupportLevel::Supported {
        None
    } else {
        Some(Blocked::Capability)
    };
    Verdict {
        level,
        note: claude.and_then(|c| c.note.clone()),
        blocked_by,
    }
}

pub const fn level_str(level: SupportLevel) -> &'static str {
    match level {
        SupportLevel::Supported => "supported",
        SupportLevel::Partial => "partial",
        SupportLevel::Planned => "planned",
        SupportLevel::Unsupported => "unsupported",
    }
}

/// The banner title for a capability block.
pub fn capability_title(level: SupportLevel) -> String {
    let word = match level {
        SupportLevel::Unsupported => "not supported",
        other => level_str(other),
    };
    format!("Memory editing is {word}")
}

/// One project a memory can go into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectOption {
    pub slug: String,
    pub label: String,
    /// Newest Claude session `updatedAt`, absent for vault-only projects.
    pub last_active: Option<String>,
}

/// Claude projects from session headers (newest first) and the server's
/// labels; undated projects last, ties by label. An empty `updatedAt`
/// stands for a missing one.
pub fn project_options(
    headers: &[SessionHeader],
    labels: &[ProjectLabelView],
) -> Vec<ProjectOption> {
    let mut active: Vec<(&str, Option<&str>)> = Vec::new();
    let mut at: HashMap<&str, usize> = HashMap::new();
    for h in headers.iter().filter(|h| h.agent == AgentId::Claude) {
        let date = Some(h.updated_at.as_str()).filter(|s| !s.is_empty());
        match at.get(h.project.as_str()) {
            None => {
                at.insert(&h.project, active.len());
                active.push((&h.project, date));
            }
            Some(&i) => {
                let prev = active[i].1;
                let newer = |d: &str| prev.is_none_or(|p| cmp_utf16(d, p) == Ordering::Greater);
                if prev.is_none() || date.is_some_and(newer) {
                    active[i].1 = date;
                }
            }
        }
    }
    let mut label: HashMap<&str, &str> = HashMap::new();
    let mut slugs: Vec<&str> = active.iter().map(|(s, _)| *s).collect();
    for l in labels {
        if label.insert(&l.slug, &l.project).is_none() && !at.contains_key(l.slug.as_str()) {
            slugs.push(&l.slug);
        }
    }
    let mut out: Vec<ProjectOption> = slugs
        .into_iter()
        .map(|slug| ProjectOption {
            slug: slug.to_owned(),
            label: label.get(slug).copied().unwrap_or(slug).to_owned(),
            last_active: at.get(slug).and_then(|&i| active[i].1).map(str::to_owned),
        })
        .collect();
    out.sort_by(by_recency);
    out
}

fn by_recency(a: &ProjectOption, b: &ProjectOption) -> Ordering {
    match (&a.last_active, &b.last_active) {
        (x, y) if x == y => locale_compare(&a.label, &b.label),
        (None, _) => Ordering::Greater,
        (_, None) => Ordering::Less,
        (Some(x), Some(y)) => {
            if cmp_utf16(x, y) == Ordering::Greater {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
    }
}

/// The free-text query over slug, name, description and body; no trim.
pub fn matches_query(e: &EntryView, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    let hay = format!(
        "{} {} {} {}",
        e.slug,
        e.name.as_deref().unwrap_or_default(),
        e.description.as_deref().unwrap_or_default(),
        e.body
    );
    hay.to_lowercase().contains(&q.to_lowercase())
}

/// A vault's entries the query and the type filter keep.
pub fn filter_entries<'a>(v: &'a VaultView, q: &str, ty: &str) -> Vec<&'a EntryView> {
    v.entries
        .iter()
        .filter(|e| {
            matches_query(e, q) && (ty == ALL || e.kind.as_deref().unwrap_or(UNTYPED) == ty)
        })
        .collect()
}

/// A vault on screen with the entries the filters keep.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown<'a> {
    pub vault: &'a VaultView,
    pub entries: Vec<&'a EntryView>,
}

/// The vaults in scope; with a query or a type, only those with a match.
pub fn visible_vaults<'a>(
    all: &'a AllVaultsView,
    scope: &str,
    q: &str,
    ty: &str,
) -> Vec<Shown<'a>> {
    let unfiltered = q.is_empty() && ty == ALL;
    all.vaults
        .iter()
        .filter(|v| scope == ALL || v.slug == scope)
        .map(|vault| Shown {
            vault,
            entries: filter_entries(vault, q, ty),
        })
        .filter(|s| unfiltered || !s.entries.is_empty())
        .collect()
}

/// The browse table's sortable columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortKey {
    Title,
    Type,
    Size,
    Mtime,
    InIndex,
    Links,
}

impl SortKey {
    pub const ALL: [Self; 6] = [
        Self::Title,
        Self::Type,
        Self::Size,
        Self::Mtime,
        Self::InIndex,
        Self::Links,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Type => "type",
            Self::Size => "size",
            Self::Mtime => "mtime",
            Self::InIndex => "inIndex",
            Self::Links => "links",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// A table's sort: newest first until a header is pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub asc: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self {
            key: SortKey::Mtime,
            asc: false,
        }
    }
}

impl Sort {
    /// A header press: the same column flips, a new one starts ascending.
    #[must_use]
    pub fn pressed(self, key: SortKey) -> Self {
        Self {
            key,
            asc: self.key != key || !self.asc,
        }
    }
}

/// A memory's title: frontmatter name, else slug.
pub fn title_of(e: &EntryView) -> &str {
    e.name.as_deref().unwrap_or(&e.slug)
}

fn compare(key: SortKey, a: &EntryView, b: &EntryView) -> Ordering {
    match key {
        SortKey::Title => locale_compare(title_of(a), title_of(b)),
        SortKey::Type => locale_compare(
            a.kind.as_deref().unwrap_or_default(),
            b.kind.as_deref().unwrap_or_default(),
        ),
        SortKey::Size => a.size.cmp(&b.size),
        SortKey::Mtime => a
            .mtime_ms
            .partial_cmp(&b.mtime_ms)
            .unwrap_or(Ordering::Equal),
        SortKey::InIndex => a.in_index.cmp(&b.in_index),
        SortKey::Links => a.links.len().cmp(&b.links.len()),
    }
}

/// Stable ascending sort, reversed when descending: ties then come out in
/// reverse input order, as the inspector's do.
pub fn sort_entries<'a>(entries: &[&'a EntryView], sort: Sort) -> Vec<&'a EntryView> {
    let mut out = entries.to_vec();
    out.sort_by(|a, b| compare(sort.key, a, b));
    if !sort.asc {
        out.reverse();
    }
    out
}

/// `512 B`, `1.5 KB`: 1024 per KB, never MB.
pub fn fmt_size(bytes: u64) -> String {
    #[expect(clippy::cast_precision_loss, reason = "file sizes fit f64")]
    let b = bytes as f64;
    if bytes < 1024 {
        format!("{} B", to_js_string(b))
    } else {
        format!("{} KB", to_fixed(b / 1024.0, 1))
    }
}

/// `new Date(iso).toLocaleString()` in `en-US`.
pub fn fmt_date(iso: &str, zone: &dyn LocalZone) -> String {
    parse_date(iso, zone).map_or_else(
        || "Invalid Date".to_owned(),
        |ms| to_locale_string(ms, zone),
    )
}

/// A piece of a body: text, or a `[[wikilink]]` and the slug it opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyPart<'a> {
    Text(&'a str),
    Link { text: &'a str, slug: String },
}

/// The end of a `[[...]]` match at byte `i`: `[[`, one or more non-`]`,
/// then `]]`.
fn wikilink_at(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i..i + 2) != Some(b"[[") {
        return None;
    }
    let close = b[i + 2..].iter().position(|&c| c == b']')? + i + 2;
    (close > i + 2 && b.get(close + 1) == Some(&b']')).then_some(close + 2)
}

/// `body.split(/(\[\[[^\]]+\]\])/g)`, empty pieces included, with each link
/// resolved through the entry's links (the last of a repeated `rawTarget`
/// wins), else its lower-cased text.
pub fn body_parts(e: &EntryView) -> Vec<BodyPart<'_>> {
    let targets: HashMap<&str, &str> = e
        .links
        .iter()
        .map(|l| (l.raw_target.as_str(), l.target_slug.as_str()))
        .collect();
    let body = e.body.as_str();
    let b = body.as_bytes();
    let mut out = Vec::new();
    let (mut from, mut i) = (0, 0);
    while i < b.len() {
        match wikilink_at(b, i) {
            Some(end) => {
                out.push(BodyPart::Text(&body[from..i]));
                let inner = &body[i + 2..end - 2];
                let slug = targets
                    .get(inner)
                    .map_or_else(|| inner.to_lowercase(), |s| (*s).to_owned());
                out.push(BodyPart::Link {
                    text: &body[i..end],
                    slug,
                });
                (from, i) = (end, end);
            }
            None => i += 1,
        }
    }
    out.push(BodyPart::Text(&body[from..]));
    out
}

/// CSS `text-transform: capitalize`: each word's first letter upper case.
pub fn capitalize(s: &str) -> String {
    let mut start = true;
    s.chars()
        .flat_map(|c| {
            let up = start && !c.is_whitespace();
            start = c.is_whitespace();
            if up {
                c.to_uppercase().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `13 files · indexed`.
pub fn overview_line(p: &ProjectSummaryView) -> String {
    let n = usize::try_from(p.file_count).unwrap_or(usize::MAX);
    format!(
        "{}{}",
        plural(n, "file", "files"),
        if p.has_index {
            " · indexed"
        } else {
            " · no MEMORY.md"
        }
    )
}

pub fn table_heading(shown: usize, total: usize) -> String {
    format!("{shown} of {total} memories")
}

pub fn orphan_heading(n: usize) -> String {
    format!(
        "{} (not in index)",
        plural(n, "orphan file", "orphan files")
    )
}

pub fn dangling_heading(n: usize) -> String {
    format!(
        "{} (file missing)",
        plural(n, "dangling pointer", "dangling pointers")
    )
}

/// The drawn text of the below-fold note (CSS upper-cases its tail).
pub fn below_fold_text(n: u64) -> String {
    format!(
        "{n} entr{} below the fold — INVISIBLE TO CLAUDE",
        if n == 1 { "y is" } else { "ies are" }
    )
}

pub fn empty_description(can_write: bool) -> String {
    format!(
        "No project has a memory folder with files yet. {}",
        if can_write {
            "Create the first memory in a Claude project, or let Claude save one."
        } else {
            "Memories appear here once Claude saves one."
        }
    )
}

/// The create dialog's line; `project` is the slug for a fixed target.
pub fn create_description(name: &str, project: Option<&str>) -> String {
    let name = if name.is_empty() { "<name>" } else { name };
    format!(
        "Adds {name}.md and a MEMORY.md index line in {}.",
        project.unwrap_or("<project>")
    )
}

pub fn delete_description(slug: &str, file_name: &str) -> String {
    format!(
        "Removes {file_name} and its MEMORY.md index line. Any [[{slug}]] links elsewhere will be reported as dangling."
    )
}

pub fn conflict_text(file_name: &str) -> String {
    format!(
        "{file_name} was modified after you opened it. Reload to pull the latest (your edits are discarded) or overwrite to force-save."
    )
}

/// The toast after a delete: title, description, and whether it warns.
pub fn deleted_toast(slug: &str, done: &DeleteView) -> (String, Option<String>, bool) {
    let n = done.dangling.len();
    if n == 0 {
        return (format!("Deleted {slug}"), None, false);
    }
    let items = done
        .dangling
        .iter()
        .map(|d| format!("{} → {}", d.from, d.target))
        .collect::<Vec<_>>()
        .join(", ");
    (
        format!(
            "Deleted {slug} — {} now dangling",
            plural(n, "reference", "references")
        ),
        Some(items),
        true,
    )
}

/// The text the inspector shows for a typed API error.
pub fn wire_error_message(e: &ApiError) -> String {
    match e {
        ApiError::FileChangedError { reason, .. } => {
            let r = match reason {
                peektrace_protocol::FileChangeReason::Mtime => "mtime",
                peektrace_protocol::FileChangeReason::Hash => "hash",
                peektrace_protocol::FileChangeReason::Missing => "missing",
            };
            format!("The file changed on disk ({r}) since you loaded it.")
        }
        ApiError::MemoryValidationError { reason, .. } => reason.clone(),
        ApiError::MemoryNotFoundError { project, name } => {
            format!("Memory \"{name}\" not found in {project}.")
        }
        ApiError::CapabilityUnsupportedError {
            capability_id,
            agent_id,
        } => format!("{} does not support {capability_id}.", agent_id.as_str()),
        ApiError::PathOutsideRootError { path, .. } => {
            format!("Refused: {path} is outside the agent roots.")
        }
        ApiError::SessionNotFoundError { id, .. } => format!("Session \"{id}\" was not found."),
        ApiError::TranscriptParseError { reason, .. } => format!(
            "That transcript could not be read ({reason}). The file may be locked or malformed."
        ),
        ApiError::SettingsWriteError { reason, .. } => {
            format!("Settings could not be saved ({reason}).")
        }
        ApiError::BadRequestError { reason } => {
            format!("The server refused the request ({reason}).")
        }
        ApiError::ImageNotFoundError { .. } => "That image is not in the transcript.".to_owned(),
        ApiError::NotFoundError { path } => format!("No such API route: {path}."),
        ApiError::MethodNotAllowedError { method } => {
            format!("The server does not accept {method} here.")
        }
        ApiError::StatsScanError { .. } | ApiError::StatsRowNotFoundError { .. } => {
            "The request failed.".to_owned()
        }
    }
}

#[cfg(test)]
mod tests;
