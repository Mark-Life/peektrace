//! The Memory section driven through Rune's `Runtime` on temp HOMEs built
//! by `common::memory_home`. Writes land in those temp dirs only.

mod common;
#[path = "memory/rt.rs"]
mod rt;
#[path = "memory/writes.rs"]
mod writes;

use common::memory_home::SEED;
use peektrace_protocol::{SupportLevel, WatchVersions};
use peektrace_ui::state::{Request, Route, Ui};
use peektrace_ui::state_memory::{CreateTarget, Dialog, MemoryRequest};
use peektrace_ui::views;
use rt::{
    click, has, home, mem_home, mem_runtime, now_ms, on_memory, present, runtime, settle, show,
    state, vault_slugs,
};
use rune::{Key, KeyEvent, Mods, Runtime, TextEdit, Tx};

#[test]
fn empty_home_offers_the_first_memory() {
    let rt = mem_runtime(&home::temp_home("mem-empty"));
    assert!(has(&rt, "No memories yet"));
    assert!(has(&rt, "Create the first memory"));
    assert!(present(&rt, "mem-create-anywhere"));
    assert!(!present(&rt, "mem-search"));
    assert!(!present(&rt, "mem-banner"));
    assert!(present(&rt, "mem-support"));
    assert!(has(
        &rt,
        "Memory editing: Claude. Planned: Codex, Pi, OpenCode."
    ));
    assert!(!has(&rt, "Claude projects root"));
    let agent_select = rt
        .elements()
        .into_iter()
        .filter_map(|e| e.key)
        .any(|k| k.contains("agent"));
    assert!(!agent_select, "the section has no agent selector");
}

#[test]
fn the_search_box_is_named_for_screen_readers() {
    let rt = mem_runtime(&mem_home());
    let els = rt.elements();
    let field = els
        .iter()
        .find(|e| e.key.as_deref() == Some("mem-search"))
        .map_or_else(|| panic!("no mem-search"), |e| e.path.clone());
    let named = els.iter().any(|e| {
        e.path.starts_with(&field) && e.focusable && e.label.as_deref() == Some("Search memories")
    });
    assert!(named, "the search editor reads as Search memories");
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
    assert!(present(&rt, "mem-banner"));
    assert!(has(&rt, "Read-only server"));
    assert!(has(&rt, "started with --read-only"));
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
fn entering_loads_once_and_a_memory_bump_reloads_once() {
    let mut s = state(&mem_home());
    s.pending.clear();
    s.go(Route::Memory);
    assert_eq!(loads(&s), 1, "entering the section loads the vaults");
    peektrace_ui::load_now(&mut s);
    assert!(s.memory.data.is_some());
    assert_eq!(loads(&s), 0);
    s.io = None;
    bump(&mut s, 0);
    assert_eq!(loads(&s), 0);
    bump(&mut s, 1);
    assert_eq!(loads(&s), 1);
    bump(&mut s, 1);
    assert_eq!(loads(&s), 1, "the same version does not reload again");
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
    assert!(s.memory.stale, "the held change waits");
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

#[test]
fn drawn_caches_follow_landed_data_not_requests() {
    let mut s = on_memory(&mem_home());
    let rev = s.memory.rev;
    let data = s.memory.data.clone().map_or_else(|| panic!("no data"), Ok);
    s.request_memory();
    assert_eq!(s.memory.rev, rev, "a request alone keeps the old picture");
    let rid = s.memory.rid;
    s.apply_memory(rid - 1, data.clone());
    assert_eq!(s.memory.rev, rev, "a stale reply changes nothing");
    s.apply_memory(rid, data);
    assert_eq!(s.memory.rev, rev + 1, "landed data redraws the graph");
}

#[test]
fn the_overview_fits_four_cards_a_row_at_desktop_width() {
    let rt = mem_runtime(&mem_home());
    let els = rt.elements();
    let row = els
        .iter()
        .find(|e| e.key.as_deref() == Some("mem-overview"))
        .map_or_else(|| panic!("no mem-overview"), |e| e.rect);
    let cards: Vec<[f32; 4]> = els
        .iter()
        .filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("mem-card:")))
        .map(|e| e.rect)
        .collect();
    assert!(cards.len() > 4, "the home has more projects than a row");
    let first = cards.iter().filter(|r| (r[1] - cards[0][1]).abs() < 1.0);
    assert_eq!(first.count(), 4);
    for r in &cards {
        assert!(
            r[0] + r[2] <= row[0] + row[2] + 0.5,
            "a card overflows the row"
        );
    }
}
