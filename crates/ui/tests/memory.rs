//! The Memory section driven through Rune's `Runtime` on temp HOMEs built
//! by `common::memory_home`. Writes land in those temp dirs only.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::home;
use common::memory_home::{self, SEED};
use peektrace_protocol::{SupportLevel, WatchVersions};
use peektrace_ui::source::DataSource;
use peektrace_ui::source::in_process::InProcess;
use peektrace_ui::state::{Io, Request, Route, Ui};
use peektrace_ui::state_memory::{CreateTarget, Dialog, MemoryRequest, ToastKind};
use peektrace_ui::{load_now, views};
use rune::{App, Env, Key, KeyEvent, Mods, Runtime, TextEdit, Tx};

const SIZE: (f32, f32) = (1440.0, 900.0);

fn now_ms() -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
    peektrace_js::time::parse_date(home::NOW, &peektrace_js::time::Utc).map_or(0.0, |ms| ms as f64)
}

fn state(home: &Path) -> Ui {
    let dir = home.to_string_lossy().into_owned();
    let peektrace = format!("{dir}/.peektrace");
    let source: Arc<dyn DataSource> = Arc::new(InProcess::for_home(dir, move |k| {
        (k == "PEEKTRACE_DIR").then(|| peektrace.clone())
    }));
    let handle = App::new(peektrace_ui::spec()).handle();
    let mut s = Ui::new(Some(Io { source, handle }), now_ms(), true);
    s.request_sessions();
    load_now(&mut s);
    s
}

fn settle(rt: &mut Runtime<Ui>) {
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
}

fn runtime(s: Ui) -> Runtime<Ui> {
    let mut rt = Runtime::new(s, views::view, SIZE, Env::new(true, 1.0));
    settle(&mut rt);
    rt
}

fn texts(rt: &Runtime<Ui>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter_map(|e| e.text.or(e.label))
        .collect()
}

fn mem_home() -> std::path::PathBuf {
    let h = home::temp_home("mem");
    memory_home::build_mem_home(&h);
    memory_home::seed_session(&h);
    h
}

fn on_memory(home: &Path) -> Ui {
    let mut s = state(home);
    s.go(Route::Memory);
    load_now(&mut s);
    s
}

fn has(rt: &Runtime<Ui>, needle: &str) -> bool {
    texts(rt).iter().any(|t| t.contains(needle))
}

fn present(rt: &Runtime<Ui>, key: &str) -> bool {
    rt.find(key).is_some()
}

/// Scroll the section until `key` is on screen.
fn show(rt: &mut Runtime<Ui>, key: &str) {
    let k = key.to_owned();
    rt.update(move |_, tx| tx.scroll_into_view(k));
    settle(rt);
}

fn click(rt: &mut Runtime<Ui>, key: &str) {
    show(rt, key);
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

/// Run queued loads and writes until none are left, as the app would.
fn drain(rt: &mut Runtime<Ui>) {
    for _ in 0..4 {
        rt.update(|s, _| load_now(s));
    }
    settle(rt);
}

fn mem_runtime(home: &Path) -> Runtime<Ui> {
    runtime(on_memory(home))
}

fn seed_file(home: &Path, name: &str) -> std::path::PathBuf {
    home.join(format!(".claude/projects/{SEED}/memory/{name}"))
}

fn vault_slugs(s: &Ui) -> Vec<String> {
    let m = &s.memory;
    let Some(all) = m.all() else { return vec![] };
    peektrace_ui::model::memory::visible_vaults(all, &m.scope, &m.query.text, &m.ty)
        .iter()
        .map(|v| v.vault.slug.clone())
        .collect()
}

#[test]
fn empty_home_offers_the_first_memory() {
    let rt = mem_runtime(&home::temp_home("mem-empty"));
    assert!(has(&rt, "No memories yet"));
    assert!(has(&rt, "Create the first memory"));
    assert!(present(&rt, "mem-create-anywhere"));
    assert!(!present(&rt, "mem-search"));
    assert!(!present(&rt, "mem-banner"));
    assert!(has(&rt, "Memory editing: Claude."));
}

#[test]
fn a_blocked_gate_hides_writes_and_says_why() {
    let mut s = on_memory(&home::temp_home("mem-blocked"));
    if let Some(d) = s.memory.data.as_mut() {
        for c in d.caps.iter_mut().filter(|c| c.id == "memory.crud") {
            let p = &mut c.per_agent;
            for cell in [&mut p.claude, &mut p.codex, &mut p.pi, &mut p.opencode] {
                cell.level = SupportLevel::Planned;
                cell.note = None;
            }
        }
    }
    let rt = runtime(s);
    assert!(present(&rt, "mem-banner"));
    assert!(has(&rt, "Memory editing is planned"));
    assert!(has(&rt, "Memory editing: no agent yet."));
    assert!(has(&rt, "Memories appear here once Claude saves one."));
    assert!(!present(&rt, "mem-create-anywhere"));
    assert!(!present(&rt, "mem-support"));

    let mut s = on_memory(&home::temp_home("mem-ro"));
    if let Some(d) = s.memory.data.as_mut() {
        d.read_only = true;
    }
    let rt = runtime(s);
    assert!(has(&rt, "Read-only server"));
    assert!(!has(&rt, "Memory editing is"));
    assert!(!present(&rt, "mem-create-anywhere"));
}

#[test]
fn overview_budget_and_fold() {
    let rt = mem_runtime(&mem_home());
    let projects: Vec<String> = rt
        .state
        .memory
        .all()
        .map(|a| a.projects.iter().map(|p| p.slug.clone()).collect())
        .unwrap_or_default();
    assert_eq!(
        projects.first().map(String::as_str),
        Some("-Users-demo-proj-a")
    );
    assert!(has(&rt, "13 files · indexed"));
    assert!(has(&rt, "1 file · no MEMORY.md"));
    assert!(has(&rt, "over budget"));
    assert!(has(&rt, "1 entry is below the fold — INVISIBLE TO CLAUDE"));
    assert!(present(&rt, "mem-graph:-Users-demo-seed"));
    assert!(has(&rt, "No memory files to graph."));
    assert!(has(&rt, "No typed memories yet."));
}

#[test]
fn filters_narrow_and_collapsed_vaults_stay_collapsed() {
    let mut rt = mem_runtime(&mem_home());
    let all = vault_slugs(&rt.state).len();
    rt.update(|s, _| {
        s.memory.query = TextEdit::new("note");
        "user".clone_into(&mut s.memory.ty);
    });
    settle(&mut rt);
    let narrowed = vault_slugs(&rt.state);
    assert!(
        narrowed.len() < all && narrowed.contains(&SEED.to_owned()),
        "{narrowed:?}"
    );
    show(&mut rt, &format!("mem-heading:{SEED}"));
    assert!(has(&rt, "2 of 5 memories"));
    rt.update(|s, _| s.memory.query = TextEdit::new("zzz-no-such"));
    settle(&mut rt);
    assert!(present(&rt, "mem-no-matches"));

    rt.update(|s, _| {
        s.memory.query = TextEdit::new("");
        "all".clone_into(&mut s.memory.ty);
    });
    settle(&mut rt);
    click(&mut rt, &views::memory::vault_key(SEED));
    assert!(!present(&rt, "mem-table:-Users-demo-seed"));
    rt.update(|s, _| s.memory.query = TextEdit::new("zzz-no-such"));
    settle(&mut rt);
    rt.update(|s, _| s.memory.query = TextEdit::new(""));
    settle(&mut rt);
    assert!(present(&rt, &views::memory::vault_key(SEED)));
    assert!(!present(&rt, "mem-table:-Users-demo-seed"));
}

#[test]
fn rows_links_and_graph_nodes_open_entries() {
    let mut rt = mem_runtime(&mem_home());
    let a = views::memory::row_key(SEED, "a");
    click(&mut rt, &a);
    assert!(present(&rt, "mem-detail:-Users-demo-seed:a"));
    assert!(has(&rt, "a.md"));
    click(&mut rt, &a);
    assert!(!present(&rt, "mem-detail:-Users-demo-seed:a"));

    let proj = "-Users-demo-proj-a";
    rt.update(|s, _| s.memory.toggle_row(proj, "nested"));
    settle(&mut rt);
    let chip = rt
        .elements()
        .into_iter()
        .filter_map(|e| e.key)
        .find(|k| k.starts_with(&format!("mem-link:{proj}:nested:flat:")))
        .unwrap_or_default();
    click(&mut rt, &chip);
    assert_eq!(
        rt.state.memory.expanded.get(proj).map(String::as_str),
        Some("flat")
    );

    click(&mut rt, &format!("mem-node:{SEED}:b"));
    assert_eq!(
        rt.state.memory.expanded.get(SEED).map(String::as_str),
        Some("b")
    );
    assert!(present(&rt, "mem-detail:-Users-demo-seed:b"));
}

#[test]
fn sort_headers_flip_and_ties_come_out_reversed() {
    let mut rt = mem_runtime(&mem_home());
    let order = |rt: &Runtime<Ui>| -> Vec<String> {
        let mut rows: Vec<(f32, String)> = rt
            .elements()
            .into_iter()
            .filter_map(|e| {
                let k = e.key?;
                let slug = k.strip_prefix(&format!("mem-row:{SEED}:"))?.to_owned();
                Some((e.rect[1], slug))
            })
            .collect();
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        rows.into_iter().map(|(_, s)| s).collect()
    };
    let entries: Vec<String> = rt
        .state
        .memory
        .all()
        .and_then(|a| a.vaults.iter().find(|v| v.slug == SEED))
        .map(|v| v.entries.iter().rev().map(|e| e.slug.clone()).collect())
        .unwrap_or_default();
    assert_eq!(order(&rt), entries);
    let title = format!("mem-sort:{SEED}:title");
    click(&mut rt, &title);
    let s = rt.state.memory.sort_of(SEED);
    assert!(s.asc);
    let asc = order(&rt);
    click(&mut rt, &title);
    assert!(!rt.state.memory.sort_of(SEED).asc);
    let desc = order(&rt);
    assert_eq!(asc.iter().rev().cloned().collect::<Vec<_>>(), desc);
}

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

fn loads(s: &Ui) -> usize {
    s.pending
        .iter()
        .filter(|r| matches!(r, Request::Memory(MemoryRequest::Load { .. })))
        .count()
}

fn bump(s: &mut Ui, memory: u64) {
    s.on_versions(
        WatchVersions {
            memory,
            sessions: 0,
        },
        0.0,
        &mut Tx::default(),
    );
}

#[test]
fn watch_bumps_reload_on_screen_mark_stale_off_it_and_wait_while_paused() {
    let mut s = on_memory(&mem_home());
    s.io = None;
    s.pending.clear();
    bump(&mut s, 5);
    assert_eq!(loads(&s), 0, "the first event only sets the baseline");
    bump(&mut s, 6);
    assert_eq!(loads(&s), 1);
    s.pending.clear();
    s.memory.loading = false;

    s.go(Route::Sessions);
    bump(&mut s, 7);
    assert_eq!(loads(&s), 0);
    s.go(Route::Memory);
    assert_eq!(loads(&s), 1, "a change while away reloads on return");
    s.pending.clear();
    s.memory.loading = false;
    s.go(Route::Sessions);
    s.go(Route::Memory);
    assert_eq!(loads(&s), 0, "no change, no reload");

    s.paused = true;
    bump(&mut s, 8);
    s.tick(1.0, now_ms(), &mut Tx::default());
    assert_eq!(loads(&s), 0);
    s.paused = false;
    s.tick(2.0, now_ms(), &mut Tx::default());
    assert_eq!(loads(&s), 1, "the held change applies on resume");
    s.pending.clear();
    s.memory.loading = false;
    bump(&mut s, 0);
    bump(&mut s, 1);
    assert_eq!(
        loads(&s),
        1,
        "a restarted server counts from its own baseline"
    );
}

#[test]
fn d_does_nothing_while_a_dialog_is_open() {
    let mut s = on_memory(&mem_home());
    s.io = None;
    s.memory.dialog = Some(Dialog::Create(CreateTarget::Pick));
    let mut rt = runtime(s);
    rt.on_key = Some(peektrace_ui::on_key);
    rt.key(KeyEvent {
        key: Key::Text("d".into()),
        mods: Mods::default(),
    });
    settle(&mut rt);
    assert_eq!(rt.state.dark, None);
}

#[test]
fn read_only_hides_edit_and_delete() {
    let mut s = on_memory(&mem_home());
    if let Some(d) = s.memory.data.as_mut() {
        d.read_only = true;
    }
    s.memory.toggle_row(SEED, "a");
    let rt = runtime(s);
    assert!(present(&rt, "mem-detail:-Users-demo-seed:a"));
    assert!(!present(&rt, &format!("mem-edit:{SEED}:a")));
    assert!(!present(&rt, &format!("mem-delete:{SEED}:a")));
    assert!(!present(&rt, &format!("mem-create:{SEED}")));
}

#[test]
fn a_failed_reload_keeps_the_vaults() {
    let home = mem_home();
    let mut s = on_memory(&home);
    let rid = {
        s.request_memory();
        s.memory.rid
    };
    s.pending.clear();
    s.apply_memory(
        rid,
        Err(peektrace_ui::source::LoadError::Transport {
            status: None,
            message: "disk gone".into(),
        }),
    );
    let rt = runtime(s);
    assert!(present(&rt, "mem-error"));
    assert!(present(&rt, "mem-retry"));
    assert!(present(&rt, &views::memory::vault_key(SEED)));
}
