//! Pure parsers for memory bodies and the MEMORY.md index. They take text
//! already read; the read module owns all IO.

use super::frontmatter::{parse_frontmatter, split_lines};
use super::types::{
    BudgetKind, IndexBudget, IndexEntry, IndexLink, LinkKind, LinkRef, MAX_INDEX_BYTES,
    MAX_INDEX_LINES, MemoryEntry, MemoryIndex,
};
use crate::js::regex::LazyRe;
use crate::js::text::trim;
use crate::js::{time, units};
use crate::js_re;

static ANCHOR: LazyRe = js_re!(r"#.*$", "");
static ALIAS: LazyRe = js_re!(r"\|.*$", "");
static MD_EXT: LazyRe = js_re!(r"\.[mM][dD]$", "");
static WIKI_LINK: LazyRe = js_re!(r"\[\[([^\]]+)\]\]", "g");
static MD_LINK: LazyRe = js_re!(r"\[[^\]]*\]\(([^)]+)\)", "g");
static EXTERNAL: LazyRe = js_re!(r"^[A-Za-z][A-Za-z0-9+.-]*:\/\/", "");
static BARE_BULLET: LazyRe = js_re!(r"^\s*[-*]\s+\S", "");
static HEADING: LazyRe = js_re!(r"(^|\n)#{1,6}\s", "");
static INDEX_MD: LazyRe = js_re!(
    r"^\s*[-*]\s+\[([^\]]+)\]\(([^)]+)\)\s*(?:(?:—|–|·|-{1,2})\s*(.*))?$",
    ""
);
static INDEX_WIKI: LazyRe = js_re!(
    r"^\s*[-*]\s+\[\[([^\]]+)\]\]\s*(?:(?:—|–|·|-{1,2})\s*(.*))?$",
    ""
);

/// A link or target as a comparable slug stem: no anchor, alias, `.md` or
/// directories, lowercased.
pub fn normalize_slug(target: &str) -> String {
    let stem = ANCHOR.replace_all_str(trim(target), "");
    let stem = ALIAS.replace_all_str(&stem, "");
    let stem = MD_EXT.replace_all_str(&stem, "");
    let last = stem.rsplit('/').next().unwrap_or("");
    trim(last).to_lowercase()
}

/// `fileName.replace(/\.md$/i, "")`.
pub fn strip_md(file_name: &str) -> String {
    MD_EXT.replace_all_str(file_name, "")
}

/// `[[wiki]]` and `[label](target)` links of a body, with line numbers.
fn extract_links(body: &str) -> Vec<LinkRef> {
    body.split('\n')
        .enumerate()
        .flat_map(|(i, line)| {
            let wiki = WIKI_LINK.captures_all(line).into_iter().filter_map(|m| {
                let target = m.get(1)?;
                Some(LinkRef {
                    kind: LinkKind::Wiki,
                    raw_target: target.to_owned(),
                    target_slug: normalize_slug(target),
                    line: i + 1,
                })
            });
            let md = MD_LINK.captures_all(line).into_iter().filter_map(|m| {
                let href = trim(m.get(1)?);
                (!EXTERNAL.is_match(href) && !href.starts_with('#')).then(|| LinkRef {
                    kind: LinkKind::Markdown,
                    raw_target: href.to_owned(),
                    target_slug: normalize_slug(href),
                    line: i + 1,
                })
            });
            wiki.chain(md).collect::<Vec<_>>()
        })
        .collect()
}

/// sha1 of the body lowercased with whitespace runs folded, for spotting
/// duplicates.
fn hash_body(body: &str) -> String {
    let folded = units::collapse_ws(&body.to_lowercase());
    sha1_smol::Sha1::from(trim(&folded)).digest().to_string()
}

/// The text after the last `/`.
fn basename(path: &str) -> &str {
    path.rsplit(['/', std::path::MAIN_SEPARATOR])
        .next()
        .unwrap_or(path)
}

/// A parsed entry from text already read and its mtime.
pub fn build_entry(path: &str, text: &str, mtime_ms: i64, in_index: bool) -> MemoryEntry {
    let (frontmatter, body) = parse_frontmatter(text);
    let file_name = basename(path).to_owned();
    MemoryEntry {
        slug: strip_md(&file_name),
        file_name,
        size: text.len(),
        lines: split_lines(text).len(),
        mtime: if mtime_ms > 0 {
            time::to_iso_string(mtime_ms)
        } else {
            String::new()
        },
        mtime_ms,
        body_hash: hash_body(&body),
        in_index,
        links: extract_links(&body),
        body,
        frontmatter,
    }
}

/// `m?.trim()`, dropped when empty.
fn hook(m: Option<&str>) -> Option<String> {
    m.map(trim).filter(|h| !h.is_empty()).map(str::to_owned)
}

/// One MEMORY.md line, or `None` for prose, headings and blank lines.
fn parse_index_line(raw: &str, line_number: usize) -> Option<IndexEntry> {
    if trim(raw).is_empty() {
        return None;
    }
    let entry = |link| {
        Some(IndexEntry {
            raw: raw.to_owned(),
            line_number,
            link,
        })
    };
    if let Some(m) = INDEX_MD.captures(raw)
        && let (Some(label), Some(target)) = (m.get(1), m.get(2))
    {
        return entry(IndexLink::Markdown {
            label: trim(label).to_owned(),
            target: trim(target).to_owned(),
            target_slug: normalize_slug(target),
            hook: hook(m.get(3)),
        });
    }
    if let Some(m) = INDEX_WIKI.captures(raw)
        && let Some(inner) = m.get(1)
    {
        let inner = trim(inner);
        return entry(IndexLink::Wiki {
            label: inner.to_owned(),
            target: inner.to_owned(),
            target_slug: normalize_slug(inner),
            hook: hook(m.get(2)),
        });
    }
    if BARE_BULLET.is_match(raw) {
        return entry(IndexLink::Bare);
    }
    None
}

/// MEMORY.md content as an index: its entries, whether it is one prose
/// document, and which entries fall past the 200-line / 25 KB cut.
pub fn parse_index_content(raw: &str, path: &str) -> MemoryIndex {
    let raw_lines = split_lines(raw);
    let entries: Vec<IndexEntry> = raw_lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| parse_index_line(l, i + 1))
        .collect();
    let is_monolithic = !trim(raw).is_empty()
        && entries.iter().all(|e| e.target_slug().is_none())
        && HEADING.is_match(raw);
    let offsets: Vec<usize> = raw_lines
        .iter()
        .scan(0, |at, l| {
            let start = *at;
            *at += l.len() + 1;
            Some(start)
        })
        .collect();
    let below_fold_entries = entries
        .iter()
        .filter(|e| {
            e.line_number > MAX_INDEX_LINES || offsets[e.line_number - 1] >= MAX_INDEX_BYTES
        })
        .cloned()
        .collect();
    MemoryIndex {
        path: path.to_owned(),
        raw: raw.to_owned(),
        bytes: raw.len(),
        lines: raw_lines.len(),
        over_budget: raw_lines.len() > MAX_INDEX_LINES || raw.len() > MAX_INDEX_BYTES,
        entries,
        is_monolithic,
        below_fold_entries,
    }
}

/// The budget gauge of an index, or of its absence.
pub fn index_budget(index: Option<&MemoryIndex>) -> IndexBudget {
    index.map_or(
        IndexBudget {
            lines: 0,
            bytes: 0,
            over_budget: false,
            below_fold_count: 0,
            kind: BudgetKind::Absent,
        },
        |i| IndexBudget {
            lines: i.lines,
            bytes: i.bytes,
            over_budget: i.over_budget,
            below_fold_count: i.below_fold_entries.len(),
            kind: if i.is_monolithic {
                BudgetKind::Monolithic
            } else {
                BudgetKind::Index
            },
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_targets() {
        assert_eq!(normalize_slug("Flat#section|alias"), "flat");
        assert_eq!(normalize_slug("sub/dir/Nested.md#x"), "nested");
        assert_eq!(normalize_slug("./bom.md#frag"), "bom");
        assert_eq!(normalize_slug("https://example.com/x.md"), "x");
        assert_eq!(normalize_slug("a#b\nc"), "a#b\nc");
    }

    #[test]
    fn links_in_body_order() {
        let links = extract_links(
            "see [[Flat|f]] and [x](./a.md) [y](https://e.com)\n![img](pic.png) [m](mailto:a@b) [h](#top)",
        );
        let got: Vec<(&str, &str, usize)> = links
            .iter()
            .map(|l| (l.kind.as_str(), l.target_slug.as_str(), l.line))
            .collect();
        assert_eq!(
            got,
            [
                ("wiki", "flat", 1),
                ("markdown", "a", 1),
                ("markdown", "pic.png", 2),
                ("markdown", "mailto:a@b", 2)
            ]
        );
    }

    #[test]
    fn index_lines() {
        let idx = parse_index_content(
            "# Memory\n- [Flat](flat.md) — the hook\n* [[Upper|U]]\n- bare\nprose\n",
            "/m/MEMORY.md",
        );
        assert_eq!(idx.lines, 6);
        assert_eq!(idx.entries.len(), 3);
        assert_eq!(idx.entries[0].target_slug(), Some("flat"));
        assert_eq!(idx.entries[1].target_slug(), Some("upper"));
        assert_eq!(idx.entries[2].link, IndexLink::Bare);
        assert!(!idx.is_monolithic);
        let mono = parse_index_content("## Notes\nsome prose", "p");
        assert!(mono.is_monolithic);
    }

    #[test]
    fn hook_with_line_separator_is_bare() {
        let idx = parse_index_content("- [a](a.md) — x\u{2028}y", "p");
        assert_eq!(idx.entries[0].link, IndexLink::Bare);
    }
}
