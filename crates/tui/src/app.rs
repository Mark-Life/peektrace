//! The app: the shell (tabs, footer), one state per section, and the key
//! router. Pure over its inputs: backend calls leave as jobs and come back
//! as replies, so tests drive it without a terminal or threads.

use std::sync::Arc;

use peektrace_core::watch::WatchVersions;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;

use crate::backend::{Job, Reply, Slot};
use crate::draw::{Seg, hard_wrap, put, put_lr, seg, segs_width};
use crate::screens::{Ctx, Hit, capabilities, memory, sessions, settings, stats};
use crate::theme::{self, ColorMode, fg};
use crate::widgets::{bold, key_hints};

/// Fixed inputs of a run.
#[derive(Clone)]
pub struct Config {
    /// The sibling web server URL (with the token when authed).
    pub url: String,
    pub authed: bool,
    pub color: ColorMode,
    /// Milliseconds since the epoch.
    pub now: Arc<dyn Fn() -> f64 + Send + Sync>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Sessions,
    Memory,
    Capabilities,
    Settings,
    Stats,
}

const SECTIONS: [(Section, &str); 5] = [
    (Section::Sessions, "Sessions"),
    (Section::Memory, "Memory"),
    (Section::Capabilities, "Capabilities"),
    (Section::Settings, "Settings"),
    (Section::Stats, "Stats"),
];

pub struct App {
    cfg: Config,
    pub section: Section,
    pub sessions: sessions::State,
    pub memory: memory::State,
    pub caps: capabilities::State,
    pub settings: settings::State,
    pub stats: stats::State,
    watch: WatchVersions,
    jobs: Vec<Job>,
    next_id: u64,
    quit: bool,
    size: Rect,
    hits: Vec<(Rect, Hit)>,
}

impl App {
    pub fn new(cfg: Config, size: Rect) -> Self {
        let mut app = Self {
            cfg,
            section: Section::Sessions,
            sessions: sessions::State::default(),
            memory: memory::State::default(),
            caps: capabilities::State::default(),
            settings: settings::State::default(),
            stats: stats::State::default(),
            watch: WatchVersions::default(),
            jobs: Vec::new(),
            next_id: 0,
            quit: false,
            size,
            hits: Vec::new(),
        };
        app.switch(Section::Sessions);
        app
    }

    pub const fn quit(&self) -> bool {
        self.quit
    }

    /// Calls queued since the last take.
    pub fn take_jobs(&mut self) -> Vec<Job> {
        std::mem::take(&mut self.jobs)
    }

    /// Whether a clock or spinner on screen needs a redraw each second.
    pub fn ticking(&self) -> bool {
        self.section == Section::Stats && self.stats.ticking()
    }

    pub fn resize(&mut self, size: Rect) {
        self.size = size;
    }

    fn footer_rows(&self, w: u16) -> u16 {
        if !self.cfg.authed {
            return 1;
        }
        let url = self.url_segs();
        u16::try_from(hard_wrap(&url, usize::from(w.saturating_sub(2)).max(1)).len() + 1)
            .unwrap_or(2)
    }

    fn url_segs(&self) -> Vec<Seg> {
        vec![
            seg("web ▸ ", fg(theme::ACCENT)),
            seg(self.cfg.url.clone(), fg(theme::TEXT_DIM)),
        ]
    }

    /// The section's body: under the tab row, above the footer, one cell in.
    pub fn body(&self) -> Rect {
        let s = self.size;
        let foot = self.footer_rows(s.width);
        Rect::new(
            s.x + 1,
            s.y + 1,
            s.width.saturating_sub(2),
            s.height.saturating_sub(1 + foot),
        )
    }

    fn with_cx<T>(&mut self, f: impl FnOnce(&mut Self, &mut Ctx<'_>) -> T) -> T {
        let mut jobs = std::mem::take(&mut self.jobs);
        let mut next = self.next_id;
        let area = self.body();
        let now = (self.cfg.now)();
        let out = {
            let mut cx = Ctx::new(&mut jobs, &mut next, self.watch, now, area);
            f(self, &mut cx)
        };
        self.next_id = next;
        self.jobs.extend(jobs);
        out
    }

    pub fn switch(&mut self, s: Section) {
        self.section = s;
        self.with_cx(|app, cx| match s {
            Section::Sessions => app.sessions.enter(cx),
            Section::Memory => app.memory.enter(cx),
            Section::Capabilities => app.caps.enter(),
            Section::Settings => app.settings.enter(cx),
            Section::Stats => app.stats.enter(cx),
        });
    }

    /// True while a text field owns the keyboard.
    pub const fn typing(&self) -> bool {
        match self.section {
            Section::Sessions => self.sessions.typing(),
            Section::Memory => self.memory.typing(),
            Section::Settings => self.settings.typing(),
            Section::Capabilities | Section::Stats => false,
        }
    }

    pub fn key(&mut self, k: KeyEvent) {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if !self.typing() {
            match k.code {
                KeyCode::Char('q') => {
                    self.quit = true;
                    return;
                }
                KeyCode::Char(c @ '1'..='5') => {
                    let at = usize::from(c as u8 - b'1');
                    self.switch(SECTIONS[at].0);
                    return;
                }
                _ => {}
            }
        }
        let term = self.size;
        self.with_cx(|app, cx| match app.section {
            Section::Sessions => app.sessions.key(k, cx),
            Section::Memory => app.memory.key(k),
            Section::Capabilities => app.caps.key(k),
            Section::Settings => app.settings.key(k, cx),
            Section::Stats => app.stats.key(k, cx, term),
        });
    }

    pub fn paste(&mut self, s: &str) {
        match self.section {
            Section::Sessions => self.sessions.paste(s),
            Section::Memory => self.memory.paste(s),
            Section::Settings => self.settings.paste(s),
            Section::Capabilities | Section::Stats => {}
        }
    }

    pub fn mouse(&mut self, m: MouseEvent) {
        if self.section != Section::Sessions {
            return;
        }
        let at = Position::new(m.column, m.row);
        let hit = self
            .hits
            .iter()
            .rev()
            .find(|(r, _)| r.contains(at))
            .map(|(_, h)| *h);
        let Some(hit) = hit else { return };
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.with_cx(|app, cx| app.sessions.click(hit, cx));
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let down = m.kind == MouseEventKind::ScrollDown;
                let area = self
                    .hits
                    .iter()
                    .find(|(r, h)| r.contains(at) && matches!(h, Hit::Rail | Hit::History));
                if let Some((_, h)) = area {
                    self.sessions.wheel(*h, down);
                }
            }
            _ => {}
        }
    }

    pub fn reply(&mut self, r: Reply) {
        self.with_cx(|app, cx| match r.slot {
            Slot::Sessions | Slot::Analyze | Slot::Stamp => app.sessions.reply(r, cx),
            Slot::Vaults => app.memory.reply(r),
            Slot::Settings | Slot::Save => app.settings.reply(r),
            Slot::Report | Slot::Drill | Slot::Refresh => app.stats.reply(r, cx),
        });
    }

    pub fn watch(&mut self, v: WatchVersions) {
        let old = self.watch;
        self.watch = v;
        let section = self.section;
        self.with_cx(|app, cx| {
            if v.sessions != old.sessions {
                app.sessions.watched(cx, section == Section::Sessions);
            }
            if v.memory != old.memory {
                app.memory.watched(cx, section == Section::Memory);
            }
        });
    }

    pub fn draw(&mut self, buf: &mut Buffer) {
        self.draw_with(buf, self.cfg.color);
    }

    /// Draw in a given colour mode.
    pub fn draw_with(&mut self, buf: &mut Buffer, color: ColorMode) {
        self.size = buf.area;
        let size = buf.area;
        buf.set_style(size, Style::new().bg(theme::BG).fg(theme::TEXT));
        self.hits.clear();
        if size.height < 2 || size.width < 4 {
            return;
        }
        self.header(buf);
        let body = self.body();
        let now = (self.cfg.now)();
        let mut hits = std::mem::take(&mut self.hits);
        match self.section {
            Section::Sessions => self.sessions.draw(buf, body, size, &mut hits),
            Section::Memory => self.memory.draw(buf, body),
            Section::Capabilities => self.caps.draw(buf, body),
            Section::Settings => self.settings.draw(buf, body),
            Section::Stats => self.stats.draw(buf, body, size, self.watch.sessions, now),
        }
        self.hits = hits;
        self.footer(buf);
        theme::apply(buf, color);
    }

    fn header(&self, buf: &mut Buffer) {
        let w = self.size.width;
        let right = if self.cfg.authed {
            "token"
        } else {
            "loopback only"
        };
        let tabs = |short: bool| {
            let mut segs = vec![seg("Peektrace", bold(theme::PRIMARY))];
            for (i, (s, label)) in SECTIONS.iter().enumerate() {
                segs.push(seg("  ", Style::new()));
                let text = if short {
                    format!(" {} ", i + 1)
                } else {
                    format!(" {} {label} ", i + 1)
                };
                segs.push(if *s == self.section {
                    seg(text, Style::new().fg(theme::ON_PRIMARY).bg(theme::PRIMARY))
                } else {
                    seg(text, fg(theme::TEXT_FAINT))
                });
            }
            segs
        };
        let full = tabs(false);
        let inner = w.saturating_sub(2);
        let fits = segs_width(&full) + 2 + right.len() <= usize::from(inner);
        let left = if fits { full } else { tabs(true) };
        put_lr(
            buf,
            1,
            0,
            inner,
            &left,
            &[seg(right, fg(theme::TEXT_FAINT))],
        );
    }

    fn footer(&self, buf: &mut Buffer) {
        let s = self.size;
        let inner = s.width.saturating_sub(2);
        let hints = key_hints(&[("1-5", "section"), ("q", "quit")]);
        let rows = self.footer_rows(s.width);
        let top = s.bottom().saturating_sub(rows);
        if self.cfg.authed {
            for (i, row) in hard_wrap(&self.url_segs(), usize::from(inner).max(1))
                .iter()
                .enumerate()
            {
                put(buf, 1, top + u16::try_from(i).unwrap_or(0), inner, row);
            }
            put(buf, 1, s.bottom() - 1, inner, &hints);
        } else {
            put_lr(buf, 1, top, inner, &self.url_segs(), &hints);
        }
    }
}
