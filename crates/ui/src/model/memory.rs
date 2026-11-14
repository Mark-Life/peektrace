//! The memory explorer's rules: the write gate, project options, filters,
//! sort, body links and the text the views show. Copies of the inspector's
//! `routes/memory` and `lib/memory-support` helpers: change both together.

mod support;
mod text;

use std::cmp::Ordering;
use std::collections::HashMap;

use peektrace_js::num::{to_fixed, to_js_string};
use peektrace_js::text::locale_compare;
use peektrace_js::time::{LocalZone, parse_date, to_locale_string};
use peektrace_protocol::view::memory::{AllVaultsView, EntryView, VaultView};

pub use support::*;
pub use text::*;

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

#[cfg(test)]
mod tests;
