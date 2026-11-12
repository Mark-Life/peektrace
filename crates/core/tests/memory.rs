//! Claude memory read model and writes over a real on-disk vault (port of
//! `packages/core/test/memory/memory.test.ts` and `packages/core/test/fs.test.ts`).

mod common;

use std::fs;
use std::time::{Duration, SystemTime};

use common::{TempDir, fixture};
use peektrace_core::memory::frontmatter::{compose_file, parse_frontmatter};
use peektrace_core::memory::fs::{WriteError, atomic_write, read_text, stat_mtime};
use peektrace_core::memory::{
    MemoryCtx, MemoryError, MemoryUpdate, NewMemory, VaultState, all_vaults, create, delete,
    get_vault, list_projects,
};
use peektrace_core::roots::Source;
use peektrace_core::{Agent, AgentRoots};

const SLUG_A: &str = "-tmp-proj-a";
const SLUG_B: &str = "-tmp-proj-b";

fn seed() -> TempDir {
    let d = TempDir::new("memory-vault");
    let a = format!("{SLUG_A}/memory");
    d.write(
        &format!("{a}/a.md"),
        "---\nname: a\ndescription: First note\ntype: user\n---\nLinks to [[b]] here.\n",
    );
    d.write(
        &format!("{a}/b.md"),
        "---\nname: b\ndescription: Second note\ntype: project\n---\nNo links here.\n",
    );
    d.write(
        &format!("{a}/orphan.md"),
        "---\nname: orphan\ndescription: Unindexed note\ntype: reference\n---\nAlone.\n",
    );
    d.write(
        &format!("{a}/MEMORY.md"),
        "- [A](a.md) — first\n- [B](b.md) — second\n- [Ghost](ghost.md) — missing\n",
    );
    d.write(
        &format!("{SLUG_B}/memory/c.md"),
        "---\nname: c\ndescription: Solo note\ntype: user\n---\nSolo body.\n",
    );
    d
}

fn claude(d: &TempDir) -> Vec<Source> {
    vec![Source {
        id: "claude".to_owned(),
        label: "claude".to_owned(),
        home: d.str(),
        projects_root: d.str(),
    }]
}

fn ctx(d: &TempDir) -> MemoryCtx {
    let roots = AgentRoots {
        claude: claude(d),
        ..AgentRoots::default()
    };
    MemoryCtx::new(&roots, "/nonexistent-tmp")
}

fn note(project: &str, name: &str, kind: &str) -> NewMemory {
    NewMemory {
        project: project.to_owned(),
        name: name.to_owned(),
        description: format!("about {name}"),
        kind: kind.to_owned(),
        body: format!("{name} body\n"),
    }
}

fn bump_mtime(path: &str) {
    let future = SystemTime::now() + Duration::from_secs(10);
    fs::File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(future))
        .expect("set mtime");
}

#[test]
fn list_projects_returns_every_project_with_a_non_empty_memory_dir() {
    let d = seed();
    let projects = list_projects(&claude(&d));
    let slugs: Vec<&str> = projects.iter().map(|p| p.slug.as_str()).collect();
    assert_eq!(slugs, [SLUG_A, SLUG_B]);
    assert_eq!(projects[0].file_count, 3);
    assert!(projects[0].has_index);
}

#[test]
fn get_vault_parses_entries_index_budget_diff_and_graph() {
    let d = seed();
    let v = get_vault(&claude(&d), SLUG_A);
    assert_eq!(v.state, VaultState::Ok);
    let slugs: Vec<&str> = v.entries.iter().map(|e| e.slug.as_str()).collect();
    assert_eq!(slugs, ["a", "b", "orphan"]);
    assert_eq!(v.budget.kind.as_str(), "index");
    assert!(v.diff.orphans.contains(&"orphan".to_owned()));
    assert!(v.diff.dangling.iter().any(|x| x.target.contains("ghost")));
    let edge = v.graph.edges.iter().find(|e| e.from == "a" && e.to == "b");
    assert!(edge.is_some_and(|e| e.resolved_to.is_some()));
    assert!(v.type_counts.contains(&("user".to_owned(), 1)));
}

#[test]
fn all_vaults_groups_every_memory_by_project() {
    let d = seed();
    let all = all_vaults(&claude(&d));
    assert_eq!(all.projects.len(), 2);
    assert_eq!(all.vaults.len(), 2);
    let mut labels: Vec<(&str, &str)> = all
        .labels
        .iter()
        .map(|l| (l.slug.as_str(), l.project.as_str()))
        .collect();
    labels.sort_unstable();
    assert_eq!(labels, [(SLUG_A, "a"), (SLUG_B, "b")]);
}

#[test]
fn create_writes_a_file_and_an_index_line() {
    let d = seed();
    let c = ctx(&d);
    let entry = create(&c, Agent::Claude, &note(SLUG_A, "fresh-note", "feedback")).expect("create");
    assert_eq!(entry.slug, "fresh-note");
    assert!(entry.in_index);
    let index = fs::read_to_string(d.join(&format!("{SLUG_A}/memory/MEMORY.md"))).expect("index");
    assert!(
        index.ends_with("- [fresh-note](fresh-note.md) — about fresh-note\n"),
        "{index}"
    );
    let v = get_vault(&claude(&d), SLUG_A);
    let found = v
        .entries
        .iter()
        .find(|e| e.slug == "fresh-note")
        .expect("entry");
    assert!(found.in_index);
    assert_eq!(found.kind(), Some("feedback"));
    let again = create(&c, Agent::Claude, &note(SLUG_A, "fresh-note", "user"));
    assert!(matches!(again, Err(MemoryError::Validation { .. })));
}

#[test]
fn create_rejects_bad_names_types_and_unknown_projects() {
    let d = seed();
    let c = ctx(&d);
    for m in [
        note(SLUG_A, "Not Kebab", "user"),
        note(SLUG_A, "ok", "bogus"),
        note("-tmp-proj-missing", "stray", "user"),
        note("..", "stray", "user"),
        note("a/b", "stray", "user"),
    ] {
        let r = create(&c, Agent::Claude, &m);
        assert!(matches!(r, Err(MemoryError::Validation { .. })), "{m:?}");
        assert!(!fs::exists(d.join(&format!("{}/memory/stray.md", m.project))).unwrap_or(true));
    }
    assert_eq!(
        create(&c, Agent::Claude, &note(SLUG_A, "ok", "bogus"))
            .err()
            .map(|e| e.to_string()),
        Some(
            "Invalid memory \"ok\": type must be one of user/feedback/project/reference".to_owned()
        )
    );
}

#[test]
fn create_in_a_project_without_a_memory_folder() {
    let d = seed();
    d.mkdir("-tmp-proj-new");
    create(
        &ctx(&d),
        Agent::Claude,
        &note("-tmp-proj-new", "first-note", "project"),
    )
    .expect("create");
    let index = fs::read_to_string(d.join("-tmp-proj-new/memory/MEMORY.md")).expect("index");
    assert_eq!(index, "- [first-note](first-note.md) — about first-note\n");
    let v = get_vault(&claude(&d), "-tmp-proj-new");
    let slugs: Vec<&str> = v.entries.iter().map(|e| e.slug.as_str()).collect();
    assert_eq!(slugs, ["first-note"]);
}

#[test]
fn update_with_compare_and_swap() {
    let d = seed();
    let c = ctx(&d);
    let created = create(&c, Agent::Claude, &note(SLUG_B, "cas-note", "user")).expect("create");
    let update = |mtime| MemoryUpdate {
        project: SLUG_B.to_owned(),
        name: "cas-note".to_owned(),
        body: Some("v2\n".to_owned()),
        expected_mtime: Some(mtime),
        ..MemoryUpdate::default()
    };
    let updated = update(created.mtime_ms);
    let e = peektrace_core::memory::update(&c, Agent::Claude, &updated).expect("update");
    assert_eq!(e.body.trim(), "v2");
    let path = d.join(&format!("{SLUG_B}/memory/cas-note.md"));
    fs::write(&path, "external\n").expect("write");
    bump_mtime(&path);
    let stale = peektrace_core::memory::update(&c, Agent::Claude, &update(created.mtime_ms));
    assert!(matches!(
        stale,
        Err(MemoryError::Write(WriteError::Changed { .. }))
    ));
    assert_eq!(fs::read_to_string(&path).expect("read"), "external\n");
}

#[test]
fn delete_removes_the_file_and_index_line_and_reports_dangling_refs() {
    let d = seed();
    let r = delete(&ctx(&d), Agent::Claude, SLUG_A, "b").expect("delete");
    assert_eq!(r.slug, "b");
    assert!(r.dangling.iter().any(|x| x.from == "a"));
    assert!(!fs::exists(d.join(&format!("{SLUG_A}/memory/b.md"))).unwrap_or(true));
    let index = fs::read_to_string(d.join(&format!("{SLUG_A}/memory/MEMORY.md"))).expect("index");
    assert!(!index.contains("(b.md)"));
}

#[test]
fn delete_creates_an_empty_index_when_none_exists() {
    let d = seed();
    delete(&ctx(&d), Agent::Claude, SLUG_B, "c").expect("delete");
    let index = fs::read_to_string(d.join(&format!("{SLUG_B}/memory/MEMORY.md"))).expect("index");
    assert_eq!(index, "");
    let missing = delete(&ctx(&d), Agent::Claude, SLUG_B, "c");
    assert_eq!(
        missing.err().map(|e| e.to_string()),
        Some(format!("Memory \"c\" not found in project \"{SLUG_B}\"."))
    );
}

#[test]
fn a_codex_write_is_refused() {
    let d = seed();
    let r = create(&ctx(&d), Agent::Codex, &note(SLUG_A, "codex-note", "user"));
    assert_eq!(
        r.err().map(|e| e.to_string()),
        Some("Unsupported for agent \"codex\": memory.crud".to_owned())
    );
}

#[test]
fn frontmatter_round_trip_is_byte_stable_for_both_shapes() {
    for name in ["nested.md", "flat.md"] {
        let original = fs::read_to_string(fixture(&format!("memory/{name}"))).expect("fixture");
        let (fm, body) = parse_frontmatter(&original);
        assert_eq!(compose_file(&fm, &body), original, "{name}");
    }
}

#[test]
fn reads_text_like_text_decoder() {
    let d = TempDir::new("memory-decode");
    let bom = d.write("bom.md", "\u{FEFF}---\nname: bom\n---\nx\n");
    assert_eq!(read_text(&bom).expect("read"), "---\nname: bom\n---\nx\n");
    let bad = d.join("bad.md");
    fs::write(&bad, [b'a', 0xFF, 0xFE, b'b']).expect("write");
    assert_eq!(read_text(&bad).expect("read"), "a\u{FFFD}\u{FFFD}b");
}

#[test]
fn hash_in_the_path_breaks_the_index_match() {
    let d = TempDir::new("memory-hash");
    d.write("C#proj/memory/hash.md", "---\nname: hash\n---\nx\n");
    d.write("C#proj/memory/MEMORY.md", "- [Hash](hash.md)\n");
    let v = get_vault(&claude(&d), "C#proj");
    assert!(!v.entries[0].in_index);
    assert!(v.graph.nodes[0].in_index);
    assert_eq!(v.diff.orphans, ["hash"]);
}

#[test]
fn type_counts_follow_js_key_order() {
    let d = TempDir::new("memory-types");
    for (n, t) in [
        ("a", "user"),
        ("b", "10"),
        ("c", "2"),
        ("d", "user"),
        ("e", "__proto__"),
    ] {
        d.write(
            &format!("-p/memory/{n}.md"),
            &format!("---\ntype: {t}\n---\n"),
        );
    }
    d.write("-p/memory/f.md", "no frontmatter");
    let v = get_vault(&claude(&d), "-p");
    let counts: Vec<(&str, usize)> = v
        .type_counts
        .iter()
        .map(|(k, c)| (k.as_str(), *c))
        .collect();
    assert_eq!(
        counts,
        [
            ("2", 1),
            ("10", 1),
            ("user", 2),
            ("__proto__", 1),
            ("unknown", 1)
        ]
    );
}

// fs: atomic write

#[test]
fn atomic_write_leaves_no_temp_files_and_honours_cas() {
    let d = TempDir::new("memory-fs");
    let roots = vec![d.str()];
    let path = d.join("note.md");
    atomic_write(&path, "hello", None, &roots).expect("write");
    assert_eq!(read_text(&path).expect("read"), "hello");
    let stray: Vec<String> = fs::read_dir(d.path())
        .expect("dir")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp-"))
        .collect();
    assert!(stray.is_empty(), "{stray:?}");
    let fresh = stat_mtime(&path).expect("stat");
    atomic_write(&path, "v2", Some(fresh), &roots).expect("cas write");
    fs::write(&path, "external").expect("write");
    bump_mtime(&path);
    let stale = atomic_write(&path, "v3", Some(fresh), &roots);
    assert!(matches!(stale, Err(WriteError::Changed { .. })));
    assert_eq!(read_text(&path).expect("read"), "external");
    let gone = atomic_write(&d.join("gone.md"), "x", Some(fresh), &roots);
    assert_eq!(
        gone.err().map(|e| e.to_string()),
        Some(format!(
            "File changed on disk (missing): {}. Reload and retry.",
            d.join("gone.md")
        ))
    );
}

#[test]
fn atomic_write_refuses_a_path_outside_every_root() {
    let d = TempDir::new("memory-escape");
    let r = atomic_write("/peektrace-escape.txt", "nope", None, &[d.str()]);
    assert!(matches!(r, Err(WriteError::OutsideRoots { .. })));
}
