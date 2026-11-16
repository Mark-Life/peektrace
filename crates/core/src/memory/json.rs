//! The memory model as `JSON.stringify` prints it, keys in TS order.

use super::types::{
    DeleteResult, Frontmatter, GraphData, IndexBudget, IndexEntry, IndexLink, LinkEdge,
    MAX_INDEX_BYTES, MAX_INDEX_LINES, MemoryEntry, MemoryIndex, MemoryVault, ProjectSummary,
};
use crate::js::json_out::Writer;

fn n(w: &mut Writer, x: usize) {
    w.int(i64::try_from(x).unwrap_or(i64::MAX));
}

fn strs<'a>(w: &mut Writer, items: impl IntoIterator<Item = &'a str>) {
    w.begin_arr();
    items.into_iter().for_each(|s| w.elem().str(s));
    w.end_arr();
}

fn frontmatter(w: &mut Writer, fm: &Frontmatter) {
    w.begin_obj();
    w.key("raw").str(&fm.raw);
    w.key("shape").str(fm.shape.as_str());
    w.key("extra").begin_obj();
    for (k, v) in &fm.extra {
        w.key(k).str(v);
    }
    w.end_obj();
    w.key("hadTrailingMetadataWs")
        .bool(fm.had_trailing_metadata_ws);
    w.key("descriptionQuoted").bool(fm.description_quoted);
    for (f, v) in &fm.fields {
        w.key(f.key()).str(v);
    }
    w.end_obj();
}

/// One entry at the writer's position.
pub fn write_entry(w: &mut Writer, e: &MemoryEntry) {
    w.begin_obj();
    w.key("slug").str(&e.slug);
    w.key("fileName").str(&e.file_name);
    for (k, v) in [
        ("name", e.name()),
        ("description", e.description()),
        ("type", e.kind()),
    ] {
        if let Some(v) = v {
            w.key(k).str(v);
        }
    }
    w.key("body").str(&e.body);
    n(w.key("size"), e.size);
    n(w.key("lines"), e.lines);
    w.key("mtime").str(&e.mtime);
    w.key("mtimeMs").int(e.mtime_ms);
    w.key("bodyHash").str(&e.body_hash);
    w.key("inIndex").bool(e.in_index);
    w.key("links").begin_arr();
    for l in &e.links {
        w.elem().begin_obj();
        w.key("kind").str(l.kind.as_str());
        w.key("rawTarget").str(&l.raw_target);
        w.key("targetSlug").str(&l.target_slug);
        n(w.key("line"), l.line);
        w.end_obj();
    }
    w.end_arr();
    frontmatter(w.key("frontmatter"), &e.frontmatter);
    w.end_obj();
}

fn index_entry(w: &mut Writer, e: &IndexEntry) {
    w.begin_obj();
    w.key("raw").str(&e.raw);
    n(w.key("lineNumber"), e.line_number);
    let (link, kind) = match &e.link {
        IndexLink::Markdown {
            label,
            target,
            target_slug,
            hook,
        } => (Some((label, target, target_slug, hook)), "markdown"),
        IndexLink::Wiki {
            label,
            target,
            target_slug,
            hook,
        } => (Some((label, target, target_slug, hook)), "wiki"),
        IndexLink::Bare => (None, "bare"),
    };
    if let Some((label, target, target_slug, hook)) = link {
        w.key("label").str(label);
        w.key("target").str(target);
        w.key("targetSlug").str(target_slug);
        if let Some(h) = hook {
            w.key("hook").str(h);
        }
    }
    w.key("malformed").bool(link.is_none());
    w.key("kind").str(kind);
    w.end_obj();
}

fn index_entries(w: &mut Writer, entries: &[IndexEntry]) {
    w.begin_arr();
    for e in entries {
        index_entry(w.elem(), e);
    }
    w.end_arr();
}

fn index(w: &mut Writer, i: &MemoryIndex) {
    w.begin_obj();
    w.key("exists").bool(true);
    w.key("path").str(&i.path);
    w.key("raw").str(&i.raw);
    n(w.key("bytes"), i.bytes);
    n(w.key("lines"), i.lines);
    index_entries(w.key("entries"), &i.entries);
    w.key("isMonolithic").bool(i.is_monolithic);
    w.key("overBudget").bool(i.over_budget);
    index_entries(w.key("belowFoldEntries"), &i.below_fold_entries);
    w.end_obj();
}

fn budget(w: &mut Writer, b: &IndexBudget) {
    w.begin_obj();
    n(w.key("lines"), b.lines);
    n(w.key("maxLines"), MAX_INDEX_LINES);
    n(w.key("bytes"), b.bytes);
    n(w.key("maxBytes"), MAX_INDEX_BYTES);
    w.key("overBudget").bool(b.over_budget);
    n(w.key("belowFoldCount"), b.below_fold_count);
    w.key("kind").str(b.kind.as_str());
    w.end_obj();
}

fn edge(w: &mut Writer, e: &LinkEdge) {
    w.begin_obj();
    w.key("from").str(&e.from);
    w.key("to").str(&e.to);
    w.key("kind").str(e.kind.as_str());
    w.key("resolved").bool(e.resolved_to.is_some());
    if let Some(r) = &e.resolved_to {
        w.key("resolvedTo").str(r);
    }
    if e.ambiguous {
        w.key("ambiguous").bool(true);
    }
    if let Some(c) = &e.candidates {
        strs(w.key("candidates"), c.iter().map(String::as_str));
    }
    n(w.key("line"), e.line);
    w.end_obj();
}

fn graph(w: &mut Writer, g: &GraphData) {
    w.begin_obj();
    w.key("nodes").begin_arr();
    for node in &g.nodes {
        w.elem().begin_obj();
        w.key("slug").str(&node.slug);
        w.key("type").str(&node.kind);
        n(w.key("bytes"), node.bytes);
        w.key("inIndex").bool(node.in_index);
        n(w.key("inDeg"), node.in_deg);
        n(w.key("outDeg"), node.out_deg);
        w.end_obj();
    }
    w.end_arr();
    w.key("edges").begin_arr();
    for e in &g.edges {
        edge(w.elem(), e);
    }
    w.end_arr();
    strs(w.key("orphans"), g.orphans.iter().map(String::as_str));
    w.end_obj();
}

/// One vault at the writer's position.
pub fn write_vault(w: &mut Writer, v: &MemoryVault) {
    w.begin_obj();
    w.key("slug").str(&v.slug);
    w.key("project").str(&v.project);
    w.key("memoryDir").str(&v.memory_dir);
    w.key("state").str(v.state.as_str());
    match &v.index {
        Some(i) => index(w.key("index"), i),
        None => w.key("index").null(),
    }
    w.key("entries").begin_arr();
    for e in &v.entries {
        write_entry(w.elem(), e);
    }
    w.end_arr();
    strs(
        w.key("strayFiles"),
        v.stray_files.iter().map(String::as_str),
    );
    budget(w.key("budget"), &v.budget);
    graph(w.key("graph"), &v.graph);
    w.key("diff").begin_obj();
    strs(w.key("orphans"), v.diff.orphans.iter().map(String::as_str));
    w.key("dangling").begin_arr();
    for d in &v.diff.dangling {
        w.elem().begin_obj();
        w.key("target").str(&d.target);
        n(w.key("line"), d.line);
        w.end_obj();
    }
    w.end_arr();
    w.end_obj();
    w.key("typeCounts").begin_obj();
    for (k, c) in &v.type_counts {
        n(w.key(k), *c);
    }
    w.end_obj();
    n(w.key("totalBytes"), v.total_bytes);
    w.end_obj();
}

/// `JSON.stringify(projects, null, 2)`.
pub fn projects_json(projects: &[ProjectSummary]) -> String {
    let mut w = Writer::new(true);
    w.begin_arr();
    for p in projects {
        w.elem().begin_obj();
        w.key("slug").str(&p.slug);
        w.key("project").str(&p.project);
        w.key("memoryDir").str(&p.memory_dir);
        n(w.key("fileCount"), p.file_count);
        w.key("hasIndex").bool(p.has_index);
        w.end_obj();
    }
    w.end_arr();
    w.finish()
}

/// `JSON.stringify(vault, null, 2)`.
pub fn vault_json(v: &MemoryVault) -> String {
    let mut w = Writer::new(true);
    write_vault(&mut w, v);
    w.finish()
}

/// `JSON.stringify(entry, null, 2)`.
pub fn entry_json(e: &MemoryEntry) -> String {
    let mut w = Writer::new(true);
    write_entry(&mut w, e);
    w.finish()
}

/// `JSON.stringify(result, null, 2)`.
pub fn delete_json(r: &DeleteResult) -> String {
    let mut w = Writer::new(true);
    w.begin_obj();
    w.key("slug").str(&r.slug);
    w.key("dangling").begin_arr();
    for d in &r.dangling {
        w.elem().begin_obj();
        w.key("from").str(&d.from);
        w.key("target").str(&d.target);
        n(w.key("line"), d.line);
        w.end_obj();
    }
    w.end_arr();
    w.end_obj();
    w.finish()
}
