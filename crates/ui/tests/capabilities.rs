//! The Capabilities page through Rune's `Runtime`: groups in order, a pill
//! per cell, the cell dialog and the way back to Settings.

mod common;

use std::sync::Arc;

use common::home;
use peektrace_ui::source::DataSource;
use peektrace_ui::source::in_process::InProcess;
use peektrace_ui::state::{Io, Route, Ui};
use peektrace_ui::{load_now, views};
use rune::{App, Env, Key, KeyEvent, Mods, Runtime};

const SIZE: (f32, f32) = (1440.0, 900.0);

fn runtime() -> Runtime<Ui> {
    let dir = home::temp_home("caps").to_string_lossy().into_owned();
    let source: Arc<dyn DataSource> = Arc::new(InProcess::for_home(dir, |_| None));
    let handle = App::new(peektrace_ui::spec()).handle();
    let mut s = Ui::new(Some(Io { source, handle }), 0.0, true);
    s.go(Route::Capabilities);
    load_now(&mut s);
    let mut rt = Runtime::new(s, views::view, SIZE, Env::new(true, 1.0));
    rt.on_key = Some(peektrace_ui::on_key);
    settle(&mut rt);
    rt
}

fn settle(rt: &mut Runtime<Ui>) {
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
}

fn texts(rt: &Runtime<Ui>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting)
        .filter_map(|e| e.text.or(e.label))
        .collect()
}

fn click(rt: &mut Runtime<Ui>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

fn press(rt: &mut Runtime<Ui>, key: Key) {
    rt.key(KeyEvent {
        key,
        mods: Mods::default(),
    });
    settle(rt);
}

#[test]
fn groups_rows_and_cells_in_registry_order() {
    let rt = runtime();
    let all = texts(&rt);
    let groups: Vec<&str> = all
        .iter()
        .map(String::as_str)
        .filter(|t| ["SESSIONS", "MEMORY", "MCP", "SKILLS", "HISTORY"].contains(t))
        .collect();
    assert_eq!(groups, ["SESSIONS", "MEMORY", "MCP", "SKILLS", "HISTORY"]);
    let cells = rt
        .elements()
        .iter()
        .filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("cap-cell-")))
        .count();
    assert_eq!(cells, 28);
    for head in ["Capability", "Claude", "Codex", "Pi", "OpenCode"] {
        assert!(all.iter().any(|t| t == head), "{head}");
    }
    for gloss in [
        "Built and working today.",
        "Partially built — some paths work.",
        "On the roadmap, not yet built.",
        "Not applicable / no plan.",
    ] {
        assert!(all.iter().any(|t| t == gloss), "{gloss}");
    }
    assert!(all.iter().any(|t| t == "Unsupported"));
}

#[test]
fn a_cell_opens_its_note_and_escape_closes_it() {
    let mut rt = runtime();
    click(&mut rt, "cap-cell-session.debug-context-codex");
    assert!(rt.find("cap-cell-sheet-dialog").is_some());
    assert!(
        texts(&rt)
            .iter()
            .any(|t| t
                == "Ground-truth usage + authoritative window; no on-disk memory attribution.")
    );
    assert!(texts(&rt).iter().any(|t| t == "Context debug"));
    press(&mut rt, Key::Escape);
    assert_eq!(rt.state.caps.selection, None);
    assert_eq!(
        rt.state.route,
        Route::Capabilities,
        "Escape only closed the dialog"
    );

    click(&mut rt, "cap-cell-memory.view-claude");
    let note = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("cap-cell-note") && !e.exiting)
        .and_then(|e| e.text);
    assert_eq!(note.as_deref(), Some("Built and working today."));
}

#[test]
fn escape_on_the_page_goes_back_to_settings() {
    let mut rt = runtime();
    press(&mut rt, Key::Escape);
    assert_eq!(rt.state.route, Route::Settings);
}

#[test]
fn the_matrix_loads_once() {
    let mut s = Ui::new(None, 0.0, true);
    s.go(Route::Capabilities);
    assert_eq!(s.pending.len(), 1);
    s.caps.list = Some(Vec::new());
    s.pending.clear();
    s.go(Route::Capabilities);
    assert_eq!(s.pending, []);
}

/// An open popup owns the keys, as `[role=dialog]` does in the inspector.
#[test]
fn the_cell_dialog_owns_single_keys() {
    let mut rt = runtime();
    click(&mut rt, "cap-cell-memory.crud-claude");
    assert!(rt.state.caps.selection.is_some());
    let dark = rt.state.dark;
    for k in ["d", "?"] {
        press(&mut rt, Key::Text(k.into()));
    }
    assert_eq!(rt.state.dark, dark);
    assert!(!rt.state.help_open);
    assert_eq!(rt.state.route, Route::Capabilities);
}
