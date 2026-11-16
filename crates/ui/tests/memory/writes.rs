//! Writes through the dialogs: create, edit with compare-and-swap, delete.

use std::path::Path;

use peektrace_ui::state::Request;
use peektrace_ui::state_memory::{CreateTarget, Dialog, MemoryRequest, ToastKind};
use peektrace_ui::views;
use rune::TextEdit;

use crate::common::memory_home::{self, SEED};
use crate::rt::{
    click, drain, has, mem_home, mem_runtime, now_ms, on_memory, present, runtime, seed_file,
    settle, show,
};

#[test]
fn create_shows_core_errors_then_writes_the_file_and_index() {
    let home = mem_home();
    let mut rt = mem_runtime(&home);
    click(&mut rt, &format!("mem-create:{SEED}"));
    assert!(present(&rt, "mem-create-dialog"));
    let fixed = CreateTarget::Fixed(SEED.to_owned());
    rt.update(|s, _| s.memory.draft_mut(&fixed).name = TextEdit::new("New Note"));
    settle(&mut rt);
    click(&mut rt, "mem-create-submit");
    drain(&mut rt);
    assert!(has(&rt, "name must be kebab-case"));
    rt.update(|s, _| {
        let d = s.memory.draft_mut(&fixed);
        d.name = TextEdit::new("new-note");
        d.description = TextEdit::new("Fresh");
        d.body = TextEdit::new("Body [[a]]");
    });
    settle(&mut rt);
    click(&mut rt, "mem-create-submit");
    drain(&mut rt);
    let file = std::fs::read_to_string(seed_file(&home, "new-note.md")).unwrap_or_default();
    assert!(file.contains("Body [[a]]"), "{file}");
    let index = std::fs::read_to_string(seed_file(&home, "MEMORY.md")).unwrap_or_default();
    assert!(index.contains("new-note.md"), "{index}");
    assert!(
        rt.state
            .memory
            .toasts
            .iter()
            .any(|t| t.title == "Created new-note")
    );
    assert!(!present(&rt, "mem-create-dialog"));
    assert_eq!(rt.state.memory.draft(&fixed).name.text, "");
    show(&mut rt, &format!("mem-heading:{SEED}"));
    assert!(has(&rt, "6 of 6 memories"));
    // Toasts go after their time.
    rt.update(|s, tx| {
        s.tick(10.0, now_ms(), tx);
    });
    assert_eq!(rt.state.memory.toasts, vec![]);
}

#[test]
fn create_in_a_picked_project_moves_scope_and_focus() {
    let home = mem_home();
    let mut s = on_memory(&home);
    s.memory.scope = "-Users-demo-intl".to_owned();
    s.memory.query = TextEdit::new("zz");
    let mut rt = runtime(s);
    rt.update(|s, _| {
        s.memory.dialog = Some(Dialog::Create(CreateTarget::Pick));
        let opt = peektrace_ui::model::memory::ProjectOption {
            slug: "-Users-demo-empty".to_owned(),
            label: "Users-demo-empty".to_owned(),
            last_active: None,
        };
        let d = s.memory.draft_mut(&CreateTarget::Pick);
        d.picked = Some(opt);
        d.name = TextEdit::new("first");
    });
    settle(&mut rt);
    assert!(has(
        &rt,
        "Adds first.md and a MEMORY.md index line in Users-demo-empty."
    ));
    click(&mut rt, "mem-create-submit");
    drain(&mut rt);
    let m = &rt.state.memory;
    assert_eq!(m.scope, "-Users-demo-empty");
    assert_eq!(m.query.text, "");
    assert_eq!(m.focus, None, "focus moved once the vault showed");
    assert!(present(&rt, &views::memory::vault_key("-Users-demo-empty")));
}

fn rewrite(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap_or_else(|e| panic!("{e}"));
    let later = std::time::SystemTime::UNIX_EPOCH
        + std::time::Duration::from_millis(memory_home::FIXED_MS + 60_000);
    memory_home::pin(path, later);
}

#[test]
fn edit_hits_a_conflict_then_overwrites_or_reloads() {
    let home = mem_home();
    let mut rt = mem_runtime(&home);
    rt.update(|s, _| s.memory.toggle_row(SEED, "a"));
    settle(&mut rt);
    click(&mut rt, &format!("mem-edit:{SEED}:a"));
    assert!(present(&rt, "mem-edit-dialog"));
    rewrite(
        &seed_file(&home, "a.md"),
        "---\nname: a\n---\nChanged outside.\n",
    );
    rt.update(|s, _| *s.memory.edit_text(true) = TextEdit::new("Mine."));
    click(&mut rt, "mem-edit-submit");
    drain(&mut rt);
    assert!(present(&rt, "mem-cas-conflict"));
    click(&mut rt, "mem-cas-overwrite");
    drain(&mut rt);
    let a = std::fs::read_to_string(seed_file(&home, "a.md")).unwrap_or_default();
    assert!(a.contains("Mine.") && a.contains("type: user"), "{a}");
    assert!(rt.state.memory.toasts.iter().any(|t| t.title == "Saved a"));
    assert!(!present(&rt, "mem-edit-dialog"));

    click(&mut rt, &format!("mem-edit:{SEED}:a"));
    rewrite(&seed_file(&home, "a.md"), "---\nname: a\n---\nAgain.\n");
    click(&mut rt, "mem-edit-submit");
    drain(&mut rt);
    let rid = rt.state.memory.rid;
    click(&mut rt, "mem-cas-reload");
    assert!(!present(&rt, "mem-edit-dialog"));
    assert!(rt.state.memory.rid > rid);
    drain(&mut rt);
    let body = rt
        .state
        .memory
        .all()
        .and_then(|a| a.vaults.iter().find(|v| v.slug == SEED))
        .and_then(|v| v.entries.iter().find(|e| e.slug == "a"))
        .map(|e| e.body.clone());
    assert_eq!(body.as_deref(), Some("Again.\n"));
}

#[test]
fn edit_of_an_unknown_type_shows_the_core_error() {
    let mut rt = mem_runtime(&mem_home());
    let proj = "-Users-demo-proj-a";
    rt.update(|s, _| {
        let e = s
            .memory
            .all()
            .and_then(|a| a.vaults.iter().find(|v| v.slug == proj))
            .and_then(|v| v.entries.iter().find(|e| e.slug == "weird-type"))
            .cloned();
        if let Some(e) = e {
            s.open_edit(proj, &e);
        }
    });
    settle(&mut rt);
    click(&mut rt, "mem-edit-submit");
    drain(&mut rt);
    assert!(has(
        &rt,
        "type must be one of user/feedback/project/reference"
    ));
    assert!(!present(&rt, "mem-cas-conflict"));
}

#[test]
fn delete_reports_dangling_references_and_missing_files() {
    let home = mem_home();
    let mut rt = mem_runtime(&home);
    rt.update(|s, _| s.memory.toggle_row(SEED, "b"));
    settle(&mut rt);
    let stale = rt
        .state
        .memory
        .all()
        .and_then(|a| a.vaults.iter().find(|v| v.slug == SEED))
        .and_then(|v| v.entries.iter().find(|e| e.slug == "b"))
        .cloned();
    click(&mut rt, &format!("mem-delete:{SEED}:b"));
    assert!(has(&rt, "Removes b.md and its MEMORY.md index line."));
    click(&mut rt, "mem-delete-confirm");
    drain(&mut rt);
    let t = rt.state.memory.toasts.last().cloned();
    assert_eq!(
        t.as_ref()
            .map(|t| (t.kind, t.title.as_str(), t.description.as_deref())),
        Some((
            ToastKind::Warning,
            "Deleted b — 1 reference now dangling",
            Some("a → b")
        ))
    );
    assert!(!seed_file(&home, "b.md").exists());
    let index = std::fs::read_to_string(seed_file(&home, "MEMORY.md")).unwrap_or_default();
    assert!(!index.contains("b.md"), "{index}");
    show(&mut rt, &format!("mem-heading:{SEED}"));
    assert!(has(&rt, "4 of 4 memories"));

    if let Some(e) = stale {
        rt.update(|s, _| s.open_delete(SEED, &e));
    }
    settle(&mut rt);
    click(&mut rt, "mem-delete-confirm");
    drain(&mut rt);
    let t = rt.state.memory.toasts.last().cloned();
    assert_eq!(
        t.map(|t| (t.kind, t.title)),
        Some((
            ToastKind::Error,
            "Memory \"b\" not found in -Users-demo-seed.".to_owned()
        ))
    );
    assert!(
        present(&rt, "mem-delete-dialog"),
        "a failed delete keeps the dialog"
    );
}

/// Runs every pending write, in send order, after all were sent.
fn land_writes(s: &mut peektrace_ui::state::Ui) -> Vec<(u64, bool)> {
    let source = s.io.as_ref().map(|io| io.source.clone());
    let writes: Vec<_> = std::mem::take(&mut s.pending)
        .into_iter()
        .filter_map(|r| match r {
            Request::Memory(MemoryRequest::Write { id, op }) => Some((id, op)),
            _ => None,
        })
        .collect();
    writes
        .into_iter()
        .filter_map(|(id, op)| {
            let res = source.as_ref()?.memory_write_now(&op)?;
            Some((id, s.apply_memory_write(id, res, 0.0)))
        })
        .collect()
}

#[test]
fn a_later_write_does_not_drop_an_earlier_answer() {
    let home = mem_home();
    let mut s = on_memory(&home);
    let fixed = CreateTarget::Fixed(SEED.to_owned());
    s.memory.dialog = Some(Dialog::Create(fixed.clone()));
    s.memory.draft_mut(&fixed).name = TextEdit::new("probe-note");
    s.submit_create(&fixed);
    // Cancel while the create runs, then save an edit.
    s.memory.close_create(&fixed);
    let entry = s
        .memory
        .all()
        .and_then(|a| a.vaults.iter().find(|v| v.slug == SEED))
        .and_then(|v| v.entries.iter().find(|e| e.slug == "a"))
        .cloned();
    if let Some(e) = entry {
        s.open_edit(SEED, &e);
    }
    s.submit_edit(true);
    assert_eq!(land_writes(&mut s), vec![(1, true), (2, true)]);
    assert!(seed_file(&home, "probe-note.md").exists());
    assert!(!s.memory.draft(&fixed).busy);
    let titles: Vec<_> = s.memory.toasts.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, ["Created probe-note", "Saved a"]);
    assert!(s.memory.dialog.is_none());
}

#[test]
fn an_answer_leaves_a_dialog_it_did_not_send_alone() {
    let mut s = on_memory(&mem_home());
    let entry = |s: &peektrace_ui::state::Ui, slug: &str| {
        s.memory
            .all()
            .and_then(|a| a.vaults.iter().find(|v| v.slug == SEED))
            .and_then(|v| v.entries.iter().find(|e| e.slug == slug))
            .cloned()
    };
    if let Some(e) = entry(&s, "a") {
        s.open_edit(SEED, &e);
    }
    s.submit_edit(true);
    // Cancel the running save and open a delete before its answer lands.
    s.memory.dialog = None;
    if let Some(e) = entry(&s, "b") {
        s.open_delete(SEED, &e);
    }
    assert_eq!(land_writes(&mut s), vec![(1, true)]);
    assert!(
        matches!(&s.memory.dialog, Some(Dialog::Delete(f)) if !f.busy()),
        "the delete dialog stays open and idle"
    );
}
