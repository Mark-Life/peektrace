//! Window keys through Rune's `Runtime`: the theme flip and where it is
//! kept, the shortcuts dialog, and a handler for each bound key.

mod common;

use std::sync::Arc;

use common::home;
use peektrace_ui::model::shortcuts::{SHORTCUTS, Scope, key_label};
use peektrace_ui::source::DataSource;
use peektrace_ui::source::in_process::InProcess;
use peektrace_ui::state::settings::THEME_KEY;
use peektrace_ui::state::{Io, Route, Ui};
use peektrace_ui::{MAC, load_now, views};
use rune::prefs::Prefs;
use rune::{App, Env, Key, KeyEvent, Mods, Runtime};

const SIZE: (f32, f32) = (1440.0, 900.0);

fn state() -> Ui {
    let dir = home::build_home().to_string_lossy().into_owned();
    let source: Arc<dyn DataSource> = Arc::new(InProcess::for_home(dir, |_| None));
    let handle = App::new(peektrace_ui::spec()).handle();
    let mut s = Ui::new(Some(Io { source, handle }), 0.0, true);
    s.request_sessions();
    load_now(&mut s);
    s
}

fn runtime(s: Ui, system_dark: bool) -> Runtime<Ui> {
    let mut rt = Runtime::new(s, views::view, SIZE, Env::new(system_dark, 1.0));
    rt.on_key = Some(peektrace_ui::on_key);
    settle(&mut rt);
    rt
}

fn settle(rt: &mut Runtime<Ui>) {
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
}

fn press(rt: &mut Runtime<Ui>, key: Key) {
    rt.key(KeyEvent {
        key,
        mods: Mods::default(),
    });
    settle(rt);
}

fn text_key(rt: &mut Runtime<Ui>, t: &str) {
    press(rt, Key::Text(t.into()));
}

fn label_of(rt: &Runtime<Ui>, key: &str) -> Option<String> {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
        .and_then(|e| e.label)
}

#[test]
fn d_on_a_dark_os_pins_light_then_returns_to_system_and_is_kept() {
    let file = home::temp_home("prefs").join("prefs.json");
    let mut s = state();
    s.use_prefs(Prefs::at(&file));
    let mut rt = runtime(s, true);
    assert!(rt.state.is_dark());
    assert_eq!(
        label_of(&rt, "theme-toggle").as_deref(),
        Some("Switch to light theme")
    );
    text_key(&mut rt, "d");
    assert_eq!(rt.state.dark, Some(false));
    assert_eq!(
        Prefs::at(&file).get::<String>(THEME_KEY).as_deref(),
        Some("light")
    );
    assert_eq!(
        label_of(&rt, "theme-toggle").as_deref(),
        Some("Switch to dark theme")
    );

    let mut back = state();
    back.use_prefs(Prefs::at(&file));
    assert_eq!(back.dark, Some(false), "a new window reads the pick");

    text_key(&mut rt, "d");
    assert_eq!(rt.state.dark, None);
    assert_eq!(
        Prefs::at(&file).get::<String>(THEME_KEY).as_deref(),
        Some("system")
    );
}

#[test]
fn an_unknown_stored_theme_follows_the_os() {
    let file = home::temp_home("prefs-blue").join("prefs.json");
    let mut p = Prefs::at(&file);
    p.set(THEME_KEY, "blue").expect("write prefs");
    let mut s = state();
    s.dark = Some(true);
    s.use_prefs(Prefs::at(&file));
    assert_eq!(s.dark, None);
}

#[test]
fn the_toggle_button_flips_like_d() {
    let mut rt = runtime(state(), false);
    let (x, y) = rt.find("theme-toggle").expect("toggle");
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert_eq!(rt.state.dark, Some(true));
}

#[test]
fn question_mark_lists_every_shortcut() {
    let mut rt = runtime(state(), true);
    text_key(&mut rt, "?");
    assert!(rt.state.help_open);
    assert!(rt.find("shortcuts-dialog").is_some());
    assert!(rt.find("shortcuts-list").is_some());
    let all: Vec<String> = rt
        .elements()
        .into_iter()
        .filter_map(|e| e.text.or(e.label))
        .collect();
    for s in &SHORTCUTS {
        assert!(rt.find(&format!("shortcut-{}", s.id)).is_some(), "{}", s.id);
        assert!(all.iter().any(|t| t == s.label), "{}", s.label);
    }
    let sidebar = if MAC { "⌘B" } else { "Ctrl+B" };
    assert!(all.iter().any(|t| t.contains(sidebar)), "{sidebar}");
    assert_eq!(key_label("Mod+b", MAC), sidebar);
    for heading in ["SESSIONS", "TRANSCRIPT", "GENERAL"] {
        assert!(all.iter().any(|t| t == heading), "{heading}");
    }

    let dark = rt.state.dark;
    text_key(&mut rt, "d");
    assert_eq!(rt.state.dark, dark, "the dialog owns the keys");
    press(&mut rt, Key::Escape);
    assert!(!rt.state.help_open);
}

#[test]
fn the_footer_button_opens_the_dialog() {
    let mut rt = runtime(state(), true);
    let (x, y) = rt.find("shortcuts-button").expect("button");
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(rt.state.help_open);
}

/// Every global key the native app binds does something; `[` and `]`
/// (turn steps) are listed in the dialog but not bound yet.
#[test]
fn each_bound_global_key_has_an_effect() {
    let unbound = ["prevTurn", "nextTurn"];
    for s in SHORTCUTS.iter().filter(|s| s.scope == Scope::Global) {
        let mut rt = runtime(state(), true);
        let before = (rt.state.dark, rt.state.help_open);
        let key = s.keys[0];
        text_key(&mut rt, key);
        let after = (rt.state.dark, rt.state.help_open);
        let focus_moved = rt
            .elements()
            .iter()
            .any(|e| e.focused && e.path.contains("search"));
        let acted = after != before || focus_moved;
        assert_eq!(acted, !unbound.contains(&s.id), "{} ({key})", s.id);
    }
}

#[test]
fn ctrl_or_cmd_chords_are_not_shortcuts() {
    let mut rt = runtime(state(), true);
    rt.key(KeyEvent {
        key: Key::Text("d".into()),
        mods: Mods {
            ctrl: true,
            cmd: !MAC,
            ..Mods::default()
        },
    });
    settle(&mut rt);
    assert_eq!(rt.state.dark, None);
}

#[test]
fn search_is_only_a_sessions_key() {
    let mut s = state();
    s.go(Route::Settings);
    let mut rt = runtime(s, true);
    text_key(&mut rt, "/");
    assert!(
        !rt.elements()
            .iter()
            .any(|e| e.focused && e.path.contains("search"))
    );
}

fn click(rt: &mut Runtime<Ui>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

fn rail_ids(s: &Ui) -> Vec<String> {
    s.rail
        .rows
        .iter()
        .filter_map(|&i| s.rail.header(i))
        .map(|h| h.id.clone())
        .collect()
}

fn state_without_io() -> Ui {
    let mut s = state();
    s.io = None;
    s
}

/// The DOM key names of the table as Rune's keys.
fn rune_key(dom: &str) -> Key {
    match dom {
        "ArrowDown" => Key::Down,
        "ArrowUp" => Key::Up,
        "Home" => Key::Home,
        "End" => Key::End,
        "Enter" => Key::Enter,
        t => Key::Text(t.into()),
    }
}

/// Every `list` key of the table moves the rail cursor from the second row,
/// and Enter opens the focused row: the native half of "every shortcut has
/// a handler".
#[test]
fn each_list_key_moves_the_rail_cursor() {
    let ids = rail_ids(&state_without_io());
    assert!(ids.len() >= 3, "fixture rows: {}", ids.len());
    let second = peektrace_ui::views::session_list::row_key(&ids[1]);
    let expect = |id: &str| match id {
        "listDown" => Some(ids[2].clone()),
        "listUp" | "listFirst" => Some(ids[0].clone()),
        "listLast" => ids.last().cloned(),
        _ => None,
    };
    for sc in SHORTCUTS.iter().filter(|s| s.scope == Scope::List) {
        for key in sc.keys {
            let mut rt = runtime(state_without_io(), true);
            click(&mut rt, &second);
            assert_eq!(rt.state.cursor.as_deref(), Some(ids[1].as_str()));
            press(&mut rt, rune_key(key));
            assert_eq!(rt.state.cursor, expect(sc.id), "{} ({key})", sc.id);
        }
    }

    let open = SHORTCUTS
        .iter()
        .find(|s| s.id == "listOpen")
        .expect("listOpen");
    let mut rt = runtime(state_without_io(), true);
    click(&mut rt, &second);
    press(&mut rt, rune_key(open.keys[0]));
    press(&mut rt, Key::Down);
    press(&mut rt, rune_key(open.keys[0]));
    assert_eq!(
        rt.state.selected.as_ref().map(|s| s.id.as_str()),
        Some(ids[2].as_str())
    );
}

/// The search field owns single keys, as an `<input>` does in the inspector.
#[test]
fn the_search_field_types_shortcut_keys() {
    let mut rt = runtime(state(), true);
    click(&mut rt, "search");
    for k in ["d", "?", "j"] {
        text_key(&mut rt, k);
    }
    assert_eq!(rt.state.query.text, "d?j");
    assert_eq!(rt.state.dark, None);
    assert!(!rt.state.help_open);
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.focused && e.path.contains("search"))
    );
}

/// Focus on a control that is not a text field (a session row, a button)
/// leaves single keys to the window.
#[test]
fn a_focused_control_that_is_not_a_field_leaves_the_keys() {
    let mut s = state_without_io();
    let first = rail_ids(&s)[0].clone();
    s.cursor = None;
    let mut rt = runtime(s, true);
    click(&mut rt, &peektrace_ui::views::session_list::row_key(&first));
    text_key(&mut rt, "d");
    assert_eq!(rt.state.dark, Some(false));
    text_key(&mut rt, "?");
    assert!(rt.state.help_open);
}
