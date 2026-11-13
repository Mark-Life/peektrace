//! The app shell: spec, keys, timers, scenes and load dispatch.

use std::sync::Arc;
use std::time::Duration;

use rune::{App, AppSpec, Key, KeyEvent, Tx};

use crate::source::DataSource;
use crate::state::{Io, Request, Route, Ui};
use crate::{state, views};

pub const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/geist.ttf"),
    include_bytes!("../assets/fonts/geist-mono.ttf"),
];

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
        crate::model::jstime::parse_date(&raw).map(|ms| ms as f64)
    }
    #[cfg(target_arch = "wasm32")]
    None
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
                        }
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
                }
            }
        }
    }
}

fn poll(s: &mut Ui, clock: f64, tx: &mut Tx) -> bool {
    let had = !s.pending.is_empty();
    dispatch(s);
    let changed = s.tick(clock, wall_ms(), tx);
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
    tx.focused()
        .is_some_and(|k| k == "search" || k.ends_with("/search") || k.contains("search"))
}

/// Move the rail cursor by `step` rows.
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
    tx.scroll_into_view(views::session_list::row_key(&id));
    s.cursor = Some(id);
}

pub fn on_key(s: &mut Ui, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if ev.is_command('b') {
        s.nav_collapsed = !s.nav_collapsed;
        return true;
    }
    if typing(tx)
        || ev.mods.cmd
        || ev.mods.ctrl
        || s.route != Route::Sessions && ev.key != Key::Text("d".into())
    {
        return false;
    }
    match &ev.key {
        Key::Text(t) if t == "/" => tx.focus("search", true),
        Key::Text(t) if t == "d" => s.dark = Some(!s.dark.unwrap_or(true)),
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
    ("stub-stats", |s| s.route = Route::Stats),
    ("light", |s| {
        open_scene(s);
        s.dark = Some(false);
    }),
];

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
        families: ("Geist", "Geist Mono"),
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
                s.on_versions(v, clock, tx);
            });
        }));
    }
    app
}
