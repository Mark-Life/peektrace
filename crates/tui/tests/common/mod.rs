//! Test support: the fixture HOME, a core backend over it with a pinned
//! clock and zone, inline settling, and buffer snapshots.
#![allow(dead_code)]

pub mod home;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use peektrace_core::header_cache::HeaderCache;
use peektrace_core::js::time::Utc;
use peektrace_core::roots::{RootsEnv, parse_settings};
use peektrace_core::stats::cache::cache_root;
use peektrace_core::{AgentRoots, PathStyle};
use peektrace_tui::backend::{Backend, CoreBackend, Request, Response, local_gate};
use peektrace_tui::runtime::settle;
use peektrace_tui::theme::ColorMode;
use peektrace_tui::{App, Config};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

#[allow(unused_imports, reason = "each test binary uses a part")]
pub use home::{LONG_ID, NOW_MS, build_home, empty_home};

pub fn backend(home: &Path) -> CoreBackend {
    let h = home.to_string_lossy().into_owned();
    let dir = format!("{h}/.peektrace");
    let env = RootsEnv::from_vars(h, PathStyle::NATIVE, |k| {
        (k == "PEEKTRACE_DIR").then(|| dir.clone())
    });
    let settings = std::fs::read_to_string(env.settings_path()).unwrap_or_default();
    CoreBackend {
        roots: AgentRoots::resolve(&env, &parse_settings(&settings)),
        settings_path: env.settings_path(),
        read_only: false,
        zone: Arc::new(Utc),
        headers: HeaderCache::default(),
        stats_cache_root: cache_root(&env),
        stats_gate: local_gate(),
        now: Arc::new(|| NOW_MS),
    }
}

pub fn config(authed: bool, color: ColorMode) -> Config {
    let url = if authed {
        "http://127.0.0.1:4321/?token=0123456789abcdef0123456789abcdef01234567".to_owned()
    } else {
        "http://127.0.0.1:4321".to_owned()
    };
    Config {
        url,
        authed,
        color,
        now: Arc::new(|| NOW_MS),
    }
}

/// A clock tests step by hand, in milliseconds since the epoch.
#[derive(Clone)]
pub struct Clock(Arc<std::sync::atomic::AtomicU64>);

impl Clock {
    pub fn new() -> Self {
        Self(Arc::new(NOW_MS.to_bits().into()))
    }

    pub fn advance(&self, ms: f64) {
        let next = self.now() + ms;
        self.0
            .store(next.to_bits(), std::sync::atomic::Ordering::SeqCst);
    }

    pub fn now(&self) -> f64 {
        f64::from_bits(self.0.load(std::sync::atomic::Ordering::SeqCst))
    }

    /// The default config, its clock this one.
    pub fn config(&self) -> Config {
        let me = self.clone();
        Config {
            now: Arc::new(move || me.now()),
            ..config(false, ColorMode::TrueColor)
        }
    }
}

/// Counts calls per request kind, then runs them on the inner backend.
pub struct Counting {
    pub inner: CoreBackend,
    pub calls: std::sync::Mutex<Vec<String>>,
}

impl Backend for Counting {
    fn call(&self, req: &Request) -> Response {
        let name = format!("{:?}", req.slot());
        self.calls.lock().expect("lock").push(name);
        self.inner.call(req)
    }
}

impl Counting {
    pub fn count(&self, slot: &str) -> usize {
        self.calls
            .lock()
            .expect("lock")
            .iter()
            .filter(|c| *c == slot)
            .count()
    }
}

/// An app over `home`, its first queries settled.
pub struct Harness {
    pub app: App,
    pub backend: Counting,
    pub home: PathBuf,
    pub w: u16,
    pub h: u16,
}

impl Harness {
    pub fn new(home: PathBuf, w: u16, h: u16) -> Self {
        Self::with(home, w, h, config(false, ColorMode::TrueColor))
    }

    pub fn with(home: PathBuf, w: u16, h: u16, cfg: Config) -> Self {
        let core = backend(&home);
        Self::over(home, w, h, cfg, core)
    }

    pub fn over(home: PathBuf, w: u16, h: u16, cfg: Config, inner: CoreBackend) -> Self {
        let backend = Counting {
            inner,
            calls: std::sync::Mutex::default(),
        };
        let mut app = App::new(cfg, Rect::new(0, 0, w, h));
        settle(&mut app, &backend);
        let mut me = Self {
            app,
            backend,
            home,
            w,
            h,
        };
        me.draw();
        me
    }

    pub fn settle(&mut self) {
        settle(&mut self.app, &self.backend);
    }

    pub fn draw(&mut self) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, self.w, self.h));
        self.app.draw(&mut buf);
        buf
    }

    /// Press keys (`"q"`, `"<tab>"`, `"<enter>"`, `"<esc>"`, `"<down>"`,
    /// ...), settling and drawing after each so layout-driven state updates.
    pub fn keys(&mut self, keys: &[&str]) -> &mut Self {
        for k in keys {
            self.app.key(key(k));
            self.settle();
            self.draw();
        }
        self
    }

    /// The screen as text, the temp HOME path shown as `~HOME`.
    pub fn text(&mut self) -> String {
        let home = self.home.to_string_lossy().into_owned();
        text(&self.draw()).replace(&home, "~HOME")
    }

    pub fn resize(&mut self, w: u16, h: u16) {
        self.w = w;
        self.h = h;
        self.app.resize(Rect::new(0, 0, w, h));
        self.draw();
    }
}

pub fn key(name: &str) -> KeyEvent {
    let code = match name {
        "<tab>" => KeyCode::Tab,
        "<enter>" => KeyCode::Enter,
        "<esc>" => KeyCode::Esc,
        "<down>" => KeyCode::Down,
        "<up>" => KeyCode::Up,
        "<left>" => KeyCode::Left,
        "<right>" => KeyCode::Right,
        "<home>" => KeyCode::Home,
        "<end>" => KeyCode::End,
        "<pgdn>" => KeyCode::PageDown,
        "<bs>" => KeyCode::Backspace,
        "<ctrl-c>" => return KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        "<ctrl-s>" => return KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        s => {
            let c = s.chars().next().expect("key");
            let m = if c.is_uppercase() {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::NONE
            };
            return KeyEvent::new(KeyCode::Char(c), m);
        }
    };
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Rows of symbols, trailing spaces trimmed.
pub fn text(buf: &Buffer) -> String {
    let a = buf.area;
    (a.y..a.bottom())
        .map(|y| {
            let row: String = (a.x..a.right()).map(|x| buf[(x, y)].symbol()).collect();
            row.trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Style runs per row: `fg/bg/modifiers` then the run's text.
pub fn styled(buf: &Buffer) -> String {
    let a = buf.area;
    let mut out = String::new();
    for y in a.y..a.bottom() {
        let mut last = None;
        let mut run = String::new();
        let mut line = Vec::new();
        for x in a.x..a.right() {
            let c = &buf[(x, y)];
            let key = format!("{:?}/{:?}/{:?}", c.fg, c.bg, c.modifier);
            if last.as_ref() != Some(&key) {
                if let Some(k) = last.take() {
                    line.push(format!("[{k}]{run}"));
                }
                run.clear();
                last = Some(key);
            }
            run.push_str(c.symbol());
        }
        if let Some(k) = last {
            line.push(format!("[{k}]{run}"));
        }
        out.push_str(&line.join(""));
        out.push('\n');
    }
    out
}

fn snapshot_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(name)
}

/// Compare with `tests/snapshots/<name>`; `UPDATE_SNAPSHOTS=1` rewrites it.
pub fn check(name: &str, actual: &str) {
    let path = snapshot_path(name);
    if std::env::var("UPDATE_SNAPSHOTS").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(path.parent().expect("dir")).expect("mkdir");
        std::fs::write(&path, actual).expect("write snapshot");
        return;
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing snapshot {name}; run with UPDATE_SNAPSHOTS=1"));
    if want != actual {
        let at = want.lines().zip(actual.lines()).position(|(a, b)| a != b);
        panic!(
            "snapshot {name} differs at line {}:\n want: {:?}\n  got: {:?}",
            at.map_or(0, |i| i + 1),
            at.and_then(|i| want.lines().nth(i)),
            at.and_then(|i| actual.lines().nth(i))
        );
    }
}

/// Hide the temp HOME: it differs per run and per machine, and a clipped
/// path shows a different cut of it. Each run of path text from the HOME's
/// head to the next border or style mark becomes `<HOME>`.
pub fn mask_home(text: &str, home: &str) -> String {
    let head: String = home.chars().take(8).collect();
    text.lines()
        .map(|line| {
            let Some(at) = line.find(&head) else {
                return line.to_owned();
            };
            let rest = &line[at..];
            let end = rest.find(['│', '[']).unwrap_or(rest.len());
            format!("{}<HOME> {}", &line[..at], drop_padding(&rest[end..]))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Drop a space-only styled run right after the path: its width is what
/// the HOME length leaves over.
fn drop_padding(tail: &str) -> String {
    let Some(mark) = tail.strip_prefix('[').and_then(|t| t.find(']')) else {
        return tail.to_owned();
    };
    let (style, after) = tail.split_at(mark + 2);
    let pad = after.find(['│', '[']).unwrap_or(after.len());
    if after[..pad].chars().all(|c| c == ' ') {
        format!("{style}{}", &after[pad..])
    } else {
        tail.to_owned()
    }
}

/// A text snapshot of the current screen (plus the styled one when
/// `styled`); the `NO_COLOR` variant's text must match.
pub fn snap_with(h: &mut Harness, name: &str, with_styles: bool) {
    let home = h.home.to_string_lossy().into_owned();
    let buf = h.draw();
    let plain = mask_home(&text(&buf), &home);
    check(&format!("{name}.txt"), &plain);
    if with_styles {
        check(&format!("{name}.styled"), &mask_home(&styled(&buf), &home));
    }
    let mut mono = Buffer::empty(buf.area);
    h.app.draw_with(&mut mono, ColorMode::None);
    assert_eq!(
        mask_home(&text(&mono), &home),
        plain,
        "NO_COLOR text differs for {name}"
    );
}

pub fn snap(h: &mut Harness, name: &str) {
    snap_with(h, name, false);
}

pub fn snap_styled(h: &mut Harness, name: &str) {
    snap_with(h, name, true);
}
