//! The memory read model: project discovery, one vault parsed whole
//! (entries, index, budget, diff, graph, type counts) and every vault at
//! once. Reads never fail: an unreadable part reads as missing.

use std::collections::HashSet;
use std::fs;

use tracing::info_span;

use super::fs::{exists, read_text, stat_mtime};
use super::graph::build_graph;
use super::parse::{build_entry, index_budget, normalize_slug, parse_index_content};
use super::resolve::{label_of, label_slugs};
use super::types::{
    DanglingEntry, GraphData, MemoryEntry, MemoryIndex, MemoryVault, ProjectSummary, VaultDiff,
    VaultState,
};
use crate::js::text::locale_compare;
use crate::js::value::js_key_order;
use crate::listing::default_root;
use crate::paths::PathStyle;
use crate::roots::Source;
use crate::sessions::resolve::project_slugs;

pub const INDEX_FILE: &str = "MEMORY.md";

/// A Claude project dir and its readable label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLabel {
    pub slug: String,
    pub project: String,
}

/// Every project with memory, each vault, and the label of every Claude
/// project dir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllVaults {
    pub projects: Vec<ProjectSummary>,
    pub vaults: Vec<MemoryVault>,
    pub labels: Vec<ProjectLabel>,
}

fn join(parts: &[&str]) -> String {
    PathStyle::NATIVE.join(parts)
}

/// The project's memory dir under the first Claude source that holds the
/// project, else under the default source.
pub fn memory_dir(claude: &[Source], slug: &str) -> String {
    let root = claude
        .iter()
        .map(|s| s.projects_root.as_str())
        .find(|root| exists(&join(&[root, slug])))
        .map_or_else(|| default_root(claude), str::to_owned);
    join(&[&root, slug, "memory"])
}

/// Names in `dir`, in directory order.
fn read_names(dir: &str) -> std::io::Result<Vec<String>> {
    Ok(fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect())
}

fn is_entry_name(name: &str) -> bool {
    is_markdown(name) && name != INDEX_FILE
}

#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "Claude reads only lowercase .md"
)]
fn is_markdown(name: &str) -> bool {
    name.ends_with(".md")
}

fn classify_state(entries: &[MemoryEntry], index: Option<&MemoryIndex>) -> VaultState {
    match index {
        Some(i) if i.is_monolithic => VaultState::Monolithic,
        None if entries.is_empty() => VaultState::Empty,
        _ => VaultState::Ok,
    }
}

/// Files the index misses, and index lines whose file is missing.
fn build_diff(entries: &[MemoryEntry], index: Option<&MemoryIndex>) -> VaultDiff {
    let file_slugs: HashSet<String> = entries.iter().map(|e| normalize_slug(&e.slug)).collect();
    VaultDiff {
        orphans: entries
            .iter()
            .filter(|e| !e.in_index)
            .map(|e| e.slug.clone())
            .collect(),
        dangling: index
            .map(|i| i.entries.as_slice())
            .unwrap_or_default()
            .iter()
            .filter_map(|e| {
                let slug = e.target_slug()?;
                (!file_slugs.contains(slug)).then(|| DanglingEntry {
                    target: e.target().unwrap_or(slug).to_owned(),
                    line: e.line_number,
                })
            })
            .collect(),
    }
}

/// Entries per `type`, `unknown` when unset, in JS key order.
fn type_counts(entries: &[MemoryEntry]) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for e in entries {
        let t = e.kind().unwrap_or("unknown");
        match counts.iter_mut().find(|(k, _)| k == t) {
            Some(slot) => slot.1 += 1,
            None => counts.push((t.to_owned(), 1)),
        }
    }
    js_key_order(counts)
}

/// A vault whose memory dir is not on disk.
fn absent_vault(slug: &str, project: &str, memory_dir: String) -> MemoryVault {
    MemoryVault {
        slug: slug.to_owned(),
        project: project.to_owned(),
        memory_dir,
        state: VaultState::Absent,
        index: None,
        entries: Vec::new(),
        stray_files: Vec::new(),
        budget: index_budget(None),
        graph: GraphData::default(),
        diff: VaultDiff::default(),
        type_counts: Vec::new(),
        total_bytes: 0,
    }
}

/// Slug an entry path matches in `MEMORY.md`. TS slugs the whole path, so a
/// `#` or `|` in a parent dir breaks the match; a Windows `\` does not.
fn index_key(path: &str, style: PathStyle) -> String {
    match style {
        PathStyle::Posix => normalize_slug(path),
        PathStyle::Windows => normalize_slug(&path.replace('\\', "/")),
    }
}

fn read_entry(path: &str, indexed: &HashSet<&str>) -> Option<MemoryEntry> {
    let text = read_text(path).ok()?;
    let mtime = stat_mtime(path).ok()?;
    let in_index = indexed.contains(index_key(path, PathStyle::NATIVE).as_str());
    Some(build_entry(path, &text, mtime, in_index))
}

/// One project's memory dir, parsed whole.
pub fn build_vault(claude: &[Source], slug: &str, label: &str) -> MemoryVault {
    let _span = info_span!("Memory.buildVault", slug).entered();
    let dir = memory_dir(claude, slug);
    if !exists(&dir) {
        return absent_vault(slug, label, dir);
    }
    let names = read_names(&dir).unwrap_or_default();
    let files: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|n| fs::metadata(join(&[&dir, n])).is_ok_and(|m| m.is_file()))
        .collect();
    let mut md_files: Vec<&str> = files.iter().copied().filter(|n| is_entry_name(n)).collect();
    md_files.sort_by(|a, b| locale_compare(a, b));
    let mut stray_files: Vec<String> = files
        .iter()
        .filter(|n| !is_markdown(n))
        .map(|n| (*n).to_owned())
        .collect();
    stray_files.sort_by(|a, b| locale_compare(a, b));
    let index = names
        .iter()
        .any(|n| n == INDEX_FILE)
        .then(|| join(&[&dir, INDEX_FILE]))
        .and_then(|path| {
            read_text(&path)
                .ok()
                .map(|raw| parse_index_content(&raw, &path))
        });
    let indexed: HashSet<&str> = index
        .as_ref()
        .map(|i| i.entries.iter().filter_map(|e| e.target_slug()).collect())
        .unwrap_or_default();
    let entries: Vec<MemoryEntry> = md_files
        .iter()
        .filter_map(|n| read_entry(&join(&[&dir, n]), &indexed))
        .collect();
    let index_entries = index
        .as_ref()
        .map(|i| i.entries.as_slice())
        .unwrap_or_default();
    MemoryVault {
        slug: slug.to_owned(),
        project: label.to_owned(),
        memory_dir: dir,
        state: classify_state(&entries, index.as_ref()),
        budget: index_budget(index.as_ref()),
        graph: build_graph(&entries, index_entries),
        diff: build_diff(&entries, index.as_ref()),
        type_counts: type_counts(&entries),
        total_bytes: entries.iter().map(|e| e.size).sum(),
        stray_files,
        entries,
        index,
    }
}

/// How many entry files a memory dir holds and whether it has MEMORY.md.
fn summarize_dir(dir: &str) -> (usize, bool) {
    read_names(dir).map_or((0, false), |names| {
        (
            names.iter().filter(|n| is_entry_name(n)).count(),
            names.iter().any(|n| n == INDEX_FILE),
        )
    })
}

fn summarize(
    claude: &[Source],
    slugs: &[String],
    labels: &[(String, String)],
) -> Vec<ProjectSummary> {
    let mut out: Vec<ProjectSummary> = slugs
        .iter()
        .filter_map(|slug| {
            let dir = memory_dir(claude, slug);
            if !exists(&dir) {
                return None;
            }
            let (file_count, has_index) = summarize_dir(&dir);
            (file_count > 0 || has_index).then(|| ProjectSummary {
                slug: slug.clone(),
                project: label_of(labels, slug),
                memory_dir: dir,
                file_count,
                has_index,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.file_count
            .cmp(&a.file_count)
            .then_with(|| locale_compare(&a.slug, &b.slug))
    });
    out
}

/// Every Claude project whose memory dir holds files, most files first.
pub fn list_projects(claude: &[Source]) -> Vec<ProjectSummary> {
    let _span = info_span!("Memory.listProjects").entered();
    let slugs = project_slugs(claude);
    summarize(claude, &slugs, &label_slugs(&slugs))
}

/// One project's vault, labelled against every Claude project.
pub fn get_vault(claude: &[Source], slug: &str) -> MemoryVault {
    let mut slugs = project_slugs(claude);
    if !slugs.iter().any(|s| s == slug) {
        slugs.push(slug.to_owned());
    }
    build_vault(claude, slug, &label_of(&label_slugs(&slugs), slug))
}

/// Every project with memory, each vault, and every project's label.
pub fn all_vaults(claude: &[Source]) -> AllVaults {
    let _span = info_span!("Memory.getAllVaults").entered();
    let slugs = project_slugs(claude);
    let labels = label_slugs(&slugs);
    let projects = summarize(claude, &slugs, &labels);
    let vaults = projects
        .iter()
        .map(|p| build_vault(claude, &p.slug, &p.project))
        .collect();
    AllVaults {
        projects,
        vaults,
        labels: labels
            .into_iter()
            .map(|(slug, project)| ProjectLabel { slug, project })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_key_is_the_file_slug_on_both_styles() {
        assert_eq!(
            index_key(
                r"C:\Users\x\.claude\projects\p\memory\Foo.md",
                PathStyle::Windows
            ),
            "foo"
        );
        assert_eq!(
            index_key("/home/x/.claude/projects/p/memory/Foo.md", PathStyle::Posix),
            "foo"
        );
        assert_eq!(index_key(r"C:\a\C#p\memory\x.md", PathStyle::Windows), "c");
    }
}
