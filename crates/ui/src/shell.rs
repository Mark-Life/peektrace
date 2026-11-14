//! The app shell: spec, keys, timers, scenes and load dispatch.

use std::sync::Arc;
use std::time::Duration;

use peektrace_protocol::AgentId;
use rune::{App, AppSpec, Key, KeyEvent, Mods, Tx};

use crate::model::shortcuts::{Modifiers, key_of, matches, shortcut_allowed};
use crate::source::DataSource;
use crate::state::settings::{APP_ID, FormRow};
use crate::state::{Io, Request, Route, Ui};
use crate::{state, views};

/// Keycaps and modifier rules follow the Mac.
pub const MAC: bool = cfg!(target_os = "macos");

/// Focus keys of text fields, by prefix: single keys type there.
pub const TEXT_FIELDS: &[&str] = &["search", "settings-path-", "settings-label-"];

/// Geist, plus a `DejaVu Sans` cut for the UI's own `∞ ⋯ ▴ ▾`, on every target:
/// the web has no system fonts, and native fallback picks a per-OS symbol font
/// whose widths shift the layout. Rune's embedded Inter covers Greek and
/// Cyrillic on the web.
pub const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/geist.ttf"),
    include_bytes!("../assets/fonts/geist-mono.ttf"),
    include_bytes!("../assets/fonts/symbols.ttf"),
];

const FAMILIES: (&str, &str) = ("Geist, DejaVu Sans", "Geist Mono, DejaVu Sans");

/// Wall-clock ms since 1970.
#[cfg(not(target_arch = "wasm32"))]
pub fn wall_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

#[cfg(target_arch = "wasm32")]
pub fn wall_ms() -> f64 {
    js_sys::Date::now()
}

/// The fixed "now" for tests and goldens (`PEEKTRACE_APP_NOW`, ISO).
pub fn fixed_now() -> Option<f64> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let raw = std::env::var("PEEKTRACE_APP_NOW").ok()?;
        #[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
        peektrace_js::time::parse_date(&raw, &peektrace_js::time::Utc).map(|ms| ms as f64)
    }
    #[cfg(target_arch = "wasm32")]
    None
}

/// A reply landed: one load fewer in flight, and the next ones go out.
fn landed(s: &mut Ui, tx: &mut Tx) {
    s.syncing = s.syncing.saturating_sub(1);
    if !s.pending.is_empty() {
        tx.wake_after(Duration::ZERO);
    }
}

/// Hand queued loads to the source; replies land through the app handle.
pub fn dispatch(s: &mut Ui) {
    let Some(io) = &s.io else { return };
    for req in std::mem::take(&mut s.pending) {
        s.syncing += 1;
        let h = io.handle.clone();
        match req {
            Request::Sessions { rid } => io.source.sessions(Box::new(move |res| {
                h.update(move |s, tx| {
                    s.syncing = s.syncing.saturating_sub(1);
                    if s.apply_listing(rid, res) {
                        tx.set_title(s.title());
                    }
                    if !s.pending.is_empty() {
                        tx.wake_after(Duration::ZERO);
                    }
                });
            })),
            Request::Analysis { rid, id, parent } => io.source.analysis(
                id,
                parent,
                Box::new(move |res| {
                    h.update(move |s, tx| {
                        s.syncing = s.syncing.saturating_sub(1);
                        if s.apply_analysis(rid, res) {
                            tx.set_title(s.title());
                            if let Some(key) = s.take_call_target() {
                                tx.scroll_into_view(key);
                            }
                        }
                    });
                }),
            ),
            Request::StatsReport { rid, scope } => io.source.stats_report(
                scope,
                Box::new(move |res| {
                    h.update(move |s, tx| {
                        s.apply_stats(rid, scope, res);
                        landed(s, tx);
                    });
                }),
            ),
            Request::StatsDrill { rid, scope, ask } => {
                let asked = ask.clone();
                io.source.stats_drill(
                    scope,
                    asked,
                    Box::new(move |res| {
                        h.update(move |s, tx| {
                            s.apply_drill(rid, scope, ask, res);
                            landed(s, tx);
                        });
                    }),
                );
            }
            Request::Capabilities { rid } => io.source.capabilities(Box::new(move |res| {
                h.update(move |s, tx| {
                    s.apply_capabilities(rid, res);
                    landed(s, tx);
                });
            })),
            Request::ServerInfo { rid } => io.source.server_info(Box::new(move |res| {
                h.update(move |s, tx| {
                    s.apply_server_info(rid, res);
                    landed(s, tx);
                });
            })),
            Request::Settings { rid } => io.source.settings(Box::new(move |res| {
                h.update(move |s, tx| {
                    s.apply_settings(rid, res);
                    landed(s, tx);
                });
            })),
            Request::SaveSettings { rid, update } => io.source.save_settings(
                update,
                Box::new(move |res| {
                    h.update(move |s, tx| {
                        if s.apply_save(rid, res, tx.clock())
                            && let Some(t) = &s.toast
                        {
                            tx.wake_at(t.until);
                        }
                        landed(s, tx);
                    });
                }),
            ),
        }
    }
}

/// Run queued loads at once (headless runs and scenes), so frame 0 has data.
pub fn load_now(s: &mut Ui) {
    let Some(source) = s.io.as_ref().map(|io| Arc::clone(&io.source)) else {
        return;
    };
    for req in std::mem::take(&mut s.pending) {
        match req {
            Request::Sessions { rid } => {
                if let Some(r) = source.sessions_now() {
                    s.apply_listing(rid, r);
                }
            }
            Request::Analysis { rid, id, parent } => {
                if let Some(r) = source.analysis_now(&id, parent.as_deref()) {
                    s.apply_analysis(rid, r);
                    s.take_call_target();
                }
            }
            Request::StatsReport { rid, scope } => {
                if let Some(r) = source.stats_report_now(scope) {
                    s.apply_stats(rid, scope, r);
                }
            }
            Request::StatsDrill { rid, scope, ask } => {
                if let Some(r) = source.stats_drill_now(scope, &ask) {
                    s.apply_drill(rid, scope, ask, r);
                }
            }
            Request::Capabilities { rid } => {
                if let Some(r) = source.capabilities_now() {
                    s.apply_capabilities(rid, r);
                }
            }
            Request::ServerInfo { rid } => {
                if let Some(r) = source.server_info_now() {
                    s.apply_server_info(rid, r);
                }
            }
            Request::Settings { rid } => {
                if let Some(r) = source.settings_now() {
                    s.apply_settings(rid, r);
                }
            }
            Request::SaveSettings { rid, update } => {
                if let Some(r) = source.save_settings_now(update) {
                    s.apply_save(rid, r, 0.0);
                }
            }
        }
    }
}

fn poll(s: &mut Ui, clock: f64, tx: &mut Tx) -> bool {
    let had = !s.pending.is_empty();
    dispatch(s);
    let changed = s.tick(clock, wall_ms(), tx) | s.stats_tick(clock, tx);
    #[cfg(target_arch = "wasm32")]
    if let Some(sel) = &s.selected {
        crate::web::write(sel);
    }
    if !s.now_fixed {
        tx.wake_after(Duration::from_secs_f64(state::NOW_TICK_S));
    }
    had || changed
}

fn typing(tx: &Tx) -> bool {
    tx.focused().is_some_and(|k| {
        k.split('/')
            .any(|seg| TEXT_FIELDS.iter().any(|p| seg.starts_with(p)))
            || k.contains("search")
    })
}

/// The modifiers as a browser names them. Off the Mac, a character typed
/// with Ctrl and Alt is `AltGr`.
const fn modifiers(m: Mods) -> Modifiers {
    if MAC {
        Modifiers {
            meta: m.cmd || m.logo,
            ctrl: m.ctrl,
            alt: m.alt,
            alt_graph: false,
        }
    } else {
        Modifiers {
            meta: m.logo,
            ctrl: m.ctrl || m.cmd,
            alt: m.alt,
            alt_graph: (m.ctrl || m.cmd) && m.alt,
        }
    }
}

/// Move the rail cursor, and focus with it, by `step` rows.
fn move_cursor(s: &mut Ui, step: isize, tx: &mut Tx) {
    let ids: Vec<&str> = s
        .rail
        .rows
        .iter()
        .filter_map(|&i| s.rail.header(i))
        .map(|h| h.id.as_str())
        .collect();
    if ids.is_empty() {
        return;
    }
    let at = s
        .cursor
        .as_deref()
        .and_then(|c| ids.iter().position(|i| *i == c));
    let next = match (at, step) {
        (None, _) | (Some(_), isize::MIN) => 0,
        (Some(_), isize::MAX) => ids.len() - 1,
        (Some(i), d) => i.saturating_add_signed(d).min(ids.len() - 1),
    };
    let id = ids[next].to_owned();
    let key = views::session_list::row_key(&id);
    tx.scroll_into_view(key.clone());
    tx.focus(key, true);
    s.cursor = Some(id);
}

/// Window keys: `⌘B`, `/`, `d` and `?`, whatever has focus outside a text
/// field, read by where the key sits when the layout types another symbol.
/// Escape leaves Capabilities for Settings. An open dialog owns every key.
pub fn on_key(s: &mut Ui, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if s.dialog_open() {
        return false;
    }
    if ev.is_command('b') {
        s.nav_collapsed = !s.nav_collapsed;
        return true;
    }
    if typing(tx) {
        return false;
    }
    if ev.key == Key::Escape && s.route == Route::Capabilities {
        s.go(Route::Settings);
        tx.set_title(format!("{} · Peektrace", Route::Settings.title()));
        tx.wake_after(Duration::ZERO);
        return true;
    }
    let Key::Text(typed) = &ev.key else {
        return false;
    };
    if !shortcut_allowed(modifiers(ev.mods), typed, MAC) {
        return false;
    }
    let code = tx
        .physical_key()
        .map(|c| format!("{c:?}"))
        .unwrap_or_default();
    let key = key_of(typed, &code, ev.mods.shift);
    if matches("theme", &key) {
        s.flip_theme();
    } else if matches("help", &key) {
        s.help_open = true;
    } else if matches("search", &key) && s.route == Route::Sessions {
        tx.focus("search", true);
    } else {
        return false;
    }
    true
}

/// Rail keys, heard only while a rail row has focus: j / k and the arrows
/// move the cursor, Home / End jump to the ends, Return opens the cursor.
pub fn rail_key(s: &mut Ui, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if ev.mods.cmd || ev.mods.ctrl || ev.mods.alt {
        return false;
    }
    match &ev.key {
        Key::Text(t) if t == "j" => move_cursor(s, 1, tx),
        Key::Text(t) if t == "k" => move_cursor(s, -1, tx),
        Key::Down => move_cursor(s, 1, tx),
        Key::Up => move_cursor(s, -1, tx),
        Key::Home => move_cursor(s, isize::MIN, tx),
        Key::End => move_cursor(s, isize::MAX, tx),
        Key::Enter => {
            let Some(id) = s.cursor.clone() else {
                return false;
            };
            s.select(&id, None);
            tx.set_title(s.title());
            tx.wake_after(Duration::ZERO);
        }
        _ => return false,
    }
    true
}

/// Scenes for headless goldens. Detail scenes open the newest session, or
/// the one an extra `session=<id>` argument names.
/// A named starting state.
pub type Scene = (&'static str, fn(&mut Ui));

pub const SCENES: &[Scene] = &[
    ("list", |_| {}),
    ("list-filtered", |s| {
        s.query = rune::TextEdit::new("refactor");
        s.apply_query();
    }),
    ("detail-top", open_scene),
    ("detail-chart-hover", |s| {
        open_scene(s);
        s.hover = s
            .detail
            .as_ref()
            .and_then(|d| d.a.snapshots.len().checked_sub(1))
            .map(|n| n / 2);
    }),
    ("history-open", |s| {
        open_scene(s);
        s.expand_all();
    }),
    ("light", |s| {
        open_scene(s);
        s.dark = Some(false);
    }),
    ("nav-collapsed", |s| s.nav_collapsed = true),
    ("stats", stats_scene),
    ("stats-pending", |s| s.enter_stats(0.0)),
    ("stats-cause-open", |s| {
        stats_scene(s);
        s.stats.open_cause = first_cause(s);
    }),
    ("stats-drill", |s| {
        stats_scene(s);
        if let Some(key) = first_cause(s) {
            s.open_drill(peektrace_protocol::DrillTable::Cause, &key);
            load_now(s);
        }
    }),
    ("stats-findings-all", |s| {
        stats_scene(s);
        s.stats.show_all = true;
    }),
    ("stats-light", |s| {
        stats_scene(s);
        s.dark = Some(false);
    }),
    ("settings", settings_scene),
    ("settings-rows", |s| {
        settings_scene(s);
        s.settings.rows[0] = vec![
            FormRow {
                path: rune::TextEdit::new("~/work/.claude"),
                label: rune::TextEdit::new("work"),
            },
            FormRow::default(),
        ];
    }),
    ("settings-conflict", |s| {
        settings_scene(s);
        s.settings.conflict = true;
    }),
    ("settings-readonly", |s| {
        settings_scene(s);
        if let Some(v) = &mut s.info.value {
            v.read_only = true;
        }
    }),
    ("capabilities", capabilities_scene),
    ("capabilities-cell", |s| {
        capabilities_scene(s);
        s.open_cell("session.debug-context", AgentId::Codex);
    }),
    ("shortcuts", |s| s.help_open = true),
];

fn stats_scene(s: &mut Ui) {
    s.enter_stats(0.0);
    load_now(s);
}

fn first_cause(s: &Ui) -> Option<String> {
    s.stats
        .screen()
        .and_then(|x| x.report.causes.first())
        .map(|c| c.key.clone())
}

fn settings_scene(s: &mut Ui) {
    s.go(Route::Settings);
    load_now(s);
}

fn capabilities_scene(s: &mut Ui) {
    s.go(Route::Capabilities);
    load_now(s);
}

fn scene_session(s: &Ui) -> Option<String> {
    rune::extra_args()
        .iter()
        .find_map(|a| a.strip_prefix("session=").map(str::to_owned))
        .or_else(|| {
            s.rail
                .rows
                .first()
                .and_then(|&i| s.rail.header(i))
                .map(|h| h.id.clone())
        })
}

fn open_scene(s: &mut Ui) {
    if let Some(id) = scene_session(s) {
        s.select(&id, None);
        load_now(s);
    }
}

fn spec_init() -> Ui {
    Ui::new(
        None,
        fixed_now().unwrap_or_else(wall_ms),
        fixed_now().is_some(),
    )
}

/// The app spec: fonts, keys, timers and scenes.
pub fn spec() -> AppSpec<Ui> {
    AppSpec {
        fonts: FONTS,
        families: FAMILIES,
        poll: Some(poll),
        on_key: Some(on_key),
        scenes: SCENES,
        ..AppSpec::new("Peektrace", (1440.0, 900.0), spec_init, views::view)
    }
}

/// The app wired to `source`: listing on start (at once when headless),
/// live refresh from the source's watch.
pub fn app(source: &Arc<dyn DataSource>) -> App<Ui> {
    let app = App::new(spec());
    let handle = app.handle();
    let watch_handle = handle.clone();
    let now = fixed_now();
    let headless = rune::is_headless();
    let init_source = Arc::clone(source);
    let app = app.init(move || {
        let mut s = Ui::new(
            Some(Io {
                source: init_source,
                handle,
            }),
            now.unwrap_or_else(wall_ms),
            now.is_some(),
        );
        if !headless {
            s.use_prefs(rune::prefs::Prefs::open(APP_ID));
        }
        s.request_sessions();
        #[cfg(target_arch = "wasm32")]
        if let Some(sel) = crate::web::read() {
            s.select(&sel.id, sel.parent);
        }
        if headless {
            load_now(&mut s);
        }
        s
    });
    if !headless {
        source.watch(Box::new(move |v| {
            watch_handle.update(move |s, tx| {
                let clock = tx.clock();
                s.on_watch(v, clock, tx);
            });
        }));
    }
    app
}
