//! Stats: what fails and what costs time, from the cached report. A file
//! change marks the report stale; `R` re-reads it and `F` re-extracts.

mod drill;
pub mod layout;
mod tables;
pub mod text;

use peektrace_core::Agent;
use peektrace_core::stats::findings::Finding;
use peektrace_core::stats::report::StatsReport;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;

use self::drill::{Drill, DrillOut};
use self::layout::{MIN_HEIGHT, MIN_WIDTH, columns_of, content_width, layout_of};
use self::text::{
    DrillTarget, Pane, ago_of, caveat_lines, corpus_line, fail_target, finding_target, findings_of,
    note_strip_of, provenance_line, scope_text, time_target,
};
use super::{Ctx, Query};
use crate::backend::{Reply, Request, Response};
use crate::draw::{panel, put, put_lr, seg};
use crate::fmt::{clip, fmt, width};
use crate::list::{ListSel, window_slice};
use crate::theme::{self, fg};
use crate::widgets::{bold, error_line, key_hints, loading};

const TITLE: &str = "Stats ▸ what fails, what costs time";
const DAYS: [u32; 4] = [7, 30, 60, 90];
const OVERLAY_CHROME: usize = 5;

#[derive(Debug)]
pub struct State {
    days: u32,
    agent: Option<Agent>,
    pane: Pane,
    collapsed: bool,
    caveats: bool,
    caveat_sel: ListSel,
    drill: Option<Drill>,
    extracting: Option<String>,
    refresh: Option<u64>,
    q: Query<Box<StatsReport>>,
    findings: Vec<Finding>,
    asked_at: u64,
    scanned: u64,
    sels: [ListSel; 3],
    loaded: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            days: 60,
            agent: None,
            pane: Pane::Findings,
            collapsed: false,
            caveats: false,
            caveat_sel: ListSel::default(),
            drill: None,
            extracting: None,
            refresh: None,
            q: Query::default(),
            findings: Vec::new(),
            asked_at: 0,
            scanned: 0,
            sels: [ListSel::default(); 3],
            loaded: false,
        }
    }
}

const fn pane_at(p: Pane) -> usize {
    match p {
        Pane::Findings => 0,
        Pane::Fails => 1,
        Pane::Time => 2,
    }
}

impl State {
    fn report(&self) -> Option<&StatsReport> {
        self.q.data.as_deref()
    }

    fn fetch(&mut self, cx: &mut Ctx<'_>) {
        self.asked_at = cx.watch.sessions;
        let id = cx.request(Request::Report {
            days: self.days,
            agent: self.agent,
        });
        self.q.start(id);
    }

    /// The first visit reads the report; later visits keep it.
    pub fn enter(&mut self, cx: &mut Ctx<'_>) {
        if !self.loaded {
            self.loaded = true;
            self.fetch(cx);
        }
    }

    /// Whether a clock or spinner on screen needs a tick.
    pub fn ticking(&self) -> bool {
        self.q.data.is_some() || self.q.loading()
    }

    pub fn reply(&mut self, r: Reply, cx: &mut Ctx<'_>) {
        match r.resp {
            Response::Report(res) => {
                if self.q.land(r.id, res) {
                    self.scanned = self.asked_at;
                    self.findings = self.report().map(findings_of).unwrap_or_default();
                }
            }
            Response::Drill(res) => {
                if let Some(d) = &mut self.drill {
                    d.q.land(r.id, res);
                }
            }
            Response::Refreshed(res) if self.refresh == Some(r.id) => {
                self.refresh = None;
                match res {
                    Ok(()) => {
                        self.extracting = None;
                        self.fetch(cx);
                    }
                    Err(e) => self.extracting = Some(format!("re-extract failed: {e}")),
                }
            }
            _ => {}
        }
    }

    fn shown(&self, term: Rect) -> usize {
        if self.collapsed {
            0
        } else {
            layout_of(term.height - self.status_rows()).findings
        }
    }

    fn status(&self) -> Option<String> {
        self.extracting.clone().or_else(|| self.q.error.clone())
    }

    fn status_rows(&self) -> u16 {
        u16::from(self.status().is_some())
    }

    fn effective_pane(&self, term: Rect) -> Pane {
        if self.pane == Pane::Findings && self.shown(term) == 0 {
            Pane::Fails
        } else {
            self.pane
        }
    }

    fn counts(&self) -> [usize; 3] {
        let r = self.report();
        [
            self.findings.len(),
            r.map_or(0, |r| r.fails.len()),
            r.map_or(0, |r| r.time.len()),
        ]
    }

    fn target(&self, pane: Pane) -> Option<DrillTarget> {
        let r = self.report()?;
        let at = |p: Pane| self.sels[pane_at(p)].at(self.counts()[pane_at(p)]);
        match pane {
            Pane::Findings => self
                .findings
                .get(at(pane))
                .and_then(|f| finding_target(f, r)),
            Pane::Fails => r.fails.get(at(pane)).map(fail_target),
            Pane::Time => r
                .time
                .get(at(pane))
                .map(|row| time_target(row, r.corpus.bash_sec)),
        }
    }

    fn reset_sels(&mut self) {
        self.sels = [ListSel::default(); 3];
    }

    pub fn key(&mut self, k: KeyEvent, cx: &mut Ctx<'_>, term: Rect) {
        if let Some(d) = &mut self.drill {
            if d.key(k, self.days, self.agent, cx) == DrillOut::Close {
                self.drill = None;
            }
            return;
        }
        if k.code == KeyCode::Esc {
            self.caveats = false;
            return;
        }
        if self.caveats {
            if k.code == KeyCode::Char('c') {
                self.caveats = false;
            } else {
                let n = self.report().map_or(0, |r| {
                    caveat_lines(r, content_width(term.width).saturating_sub(OVERLAY_CHROME)).len()
                });
                self.caveat_sel.key(k.code, n);
            }
            return;
        }
        match k.code {
            KeyCode::Char('R') => {
                self.fetch(cx);
            }
            KeyCode::Char('F') => {
                self.extracting =
                    Some("re-extracting every transcript — a cold pass, minutes".to_owned());
                self.refresh = Some(cx.request(Request::Refresh { agent: self.agent }));
            }
            KeyCode::Tab => {
                self.pane = match self.pane {
                    Pane::Findings => Pane::Fails,
                    Pane::Fails => Pane::Time,
                    Pane::Time => Pane::Findings,
                };
            }
            KeyCode::Enter => {
                let pane = self.effective_pane(term);
                if let Some(t) = self.target(pane) {
                    self.drill = Some(Drill::open(t, self.days, self.agent, cx));
                }
            }
            KeyCode::Char('d') => {
                let at = DAYS.iter().position(|d| *d == self.days).unwrap_or(0);
                self.days = DAYS[(at + 1) % DAYS.len()];
                self.reset_sels();
                self.fetch(cx);
            }
            KeyCode::Char('a') => {
                let cycle: Vec<Option<Agent>> = std::iter::once(None)
                    .chain(Agent::ALL.into_iter().map(Some))
                    .collect();
                let at = cycle.iter().position(|a| *a == self.agent).unwrap_or(0);
                self.agent = cycle[(at + 1) % cycle.len()];
                self.reset_sels();
                self.fetch(cx);
            }
            KeyCode::Char('c') => self.caveats = true,
            KeyCode::Char('f') => self.collapsed = !self.collapsed,
            code => {
                let pane = self.effective_pane(term);
                let n = self.counts()[pane_at(pane)];
                self.sels[pane_at(pane)].key(code, n);
            }
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "header, status, body and hints in order"
    )]
    pub fn draw(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        term: Rect,
        watch_sessions: u64,
        now: f64,
    ) {
        if term.height < MIN_HEIGHT || term.width < MIN_WIDTH {
            put(
                buf,
                area.x,
                area.y,
                area.width,
                &[seg(
                    format!(
                        "terminal too small: stats needs {MIN_WIDTH}×{MIN_HEIGHT}, this one is {}×{}",
                        term.width, term.height
                    ),
                    fg(theme::WARN),
                )],
            );
            return;
        }
        let content = content_width(term.width);
        let report = self.q.data.take();
        let cols = columns_of(
            content,
            report.as_ref().is_some_and(|r| r.corpus.bash_sec > 0.0),
        );
        let ago = report
            .as_ref()
            .map_or_else(|| "…".to_owned(), |r| ago_of(r.generated_at, now));
        let stale = report.is_some() && watch_sessions > self.scanned;
        let scope = scope_text(&ago, self.agent, self.days, self.q.loading(), stale);
        let title = clip(TITLE, content.saturating_sub(width(&scope) + 2).max(12));
        put_lr(
            buf,
            area.x,
            area.y,
            area.width,
            &[seg(title, bold(theme::PRIMARY))],
            &[seg(scope, fg(theme::TEXT_DIM))],
        );
        if let Some(r) = &report {
            put(
                buf,
                area.x,
                area.y + 1,
                area.width,
                &[seg(
                    clip(&corpus_line(r, cols.wide), content),
                    fg(theme::TEXT_DIM),
                )],
            );
            put(
                buf,
                area.x,
                area.y + 2,
                area.width,
                &[seg(
                    clip(&provenance_line(r, cols.wide), content),
                    fg(theme::TEXT_FAINT),
                )],
            );
        }
        let mut y = area.y + 4;
        if let Some(line) = self.status() {
            let tone = fg(if self.extracting.is_some() {
                theme::WARN
            } else {
                theme::BAD
            });
            put(
                buf,
                area.x,
                y,
                area.width,
                &[seg(clip(&line, content), tone)],
            );
            y += 1;
        }
        let body = Rect::new(area.x, y, area.width, area.bottom().saturating_sub(y + 1));
        match &report {
            None => {
                let mut yy = body.y;
                if self.q.loading() {
                    put(
                        buf,
                        body.x,
                        yy,
                        body.width,
                        &loading(
                            "Reading the corpus — a first pass parses every transcript (minutes), later passes read the fact cache (~2s).",
                        ),
                    );
                    yy += 1;
                }
                if let Some(e) = &self.q.error {
                    put(buf, body.x, yy, body.width, &error_line(e));
                }
            }
            Some(r) => self.draw_body(buf, body, r, term, content),
        }
        self.q.data = report;
        let hints: &[(&str, &str)] = if self.drill.is_some() {
            &[("esc", "close drill")]
        } else if self.caveats {
            &[("↑↓", "scroll"), ("esc/c", "close")]
        } else if cols.wide {
            &[
                ("↑↓/jk", "move"),
                ("tab", "pane"),
                ("⏎", "drill"),
                ("d", "days"),
                ("a", "agent"),
                ("R", "re-read"),
                ("F", "re-extract"),
                ("c", "caveats"),
                ("f", "findings"),
            ]
        } else {
            &[
                ("↑↓", "move"),
                ("tab", "pane"),
                ("⏎", "drill"),
                ("d", "days"),
                ("a", "agent"),
                ("f", "findings"),
                ("R", "re-read"),
                ("c", "caveats"),
            ]
        };
        put(
            buf,
            area.x,
            area.bottom() - 1,
            area.width,
            &key_hints(hints),
        );
    }

    fn draw_body(
        &mut self,
        buf: &mut Buffer,
        body: Rect,
        r: &StatsReport,
        term: Rect,
        content: usize,
    ) {
        if let Some(d) = &mut self.drill {
            d.draw(buf, body, content);
            return;
        }
        if self.caveats {
            let lines = caveat_lines(r, content.saturating_sub(OVERLAY_CHROME));
            let inner = panel(
                buf,
                body,
                "CAVEATS  everything that travels with these numbers · esc close",
                true,
                1,
            );
            let visible = usize::from(inner.height).saturating_sub(1).max(1);
            let (start, end) = window_slice(self.caveat_sel.at(lines.len()), lines.len(), visible);
            for (i, line) in lines[start..end].iter().enumerate() {
                put(
                    buf,
                    inner.x,
                    inner.y + u16::try_from(i).unwrap_or(0),
                    inner.width,
                    &[seg(line.clone(), fg(theme::TEXT_DIM))],
                );
            }
            #[expect(clippy::cast_precision_loss, reason = "line counts")]
            let more = if end < lines.len() {
                format!("… {} more · ↑↓ scroll", fmt((lines.len() - end) as f64))
            } else {
                " ".to_owned()
            };
            if inner.height > 0 {
                put(
                    buf,
                    inner.x,
                    inner.bottom() - 1,
                    inner.width,
                    &[seg(more, fg(theme::TEXT_FAINT))],
                );
            }
            return;
        }
        let layout = layout_of(term.height - self.status_rows());
        let shown = if self.collapsed { 0 } else { layout.findings };
        let pane = self.effective_pane(term);
        let cols = columns_of(content, r.corpus.bash_sec > 0.0);
        let counts = self.counts();
        let indexes = [0, 1, 2].map(|i| self.sels[i].at(counts[i]));
        let notes = note_strip_of(
            &self.findings,
            indexes[pane_at(pane)],
            pane,
            r,
            layout.note_lines,
        );
        tables::body(
            buf,
            body,
            r,
            &self.findings,
            indexes,
            pane,
            layout,
            shown,
            &notes,
            &cols,
        );
    }
}
