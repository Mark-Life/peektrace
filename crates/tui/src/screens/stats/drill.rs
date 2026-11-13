//! The drill overlay: the calls behind one ranked row, a page of 100 at a
//! time. Scrolling never fetches; `n` and `p` page.

use std::collections::HashSet;

use peektrace_core::stats::accumulate::DrillHit;
use peektrace_core::stats::format::{hms, short_id};
use peektrace_core::stats::report::DrillResult;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::text::{DrillTarget, open_command};
use crate::backend::{DRILL_PAGE, Request};
use crate::draw::{Seg, fill_row, panel, put, seg};
use crate::fmt::{clip, first_line, fmt, pad_end, pad_start, sanitize, take_width, width};
use crate::list::ListSel;
use crate::screens::{Ctx, Query};
use crate::theme::{self, fg};
use crate::widgets::{empty, error_line, key_hints, loading};

const TS_W: usize = 12;
const SID_W: usize = 14;
const PROJ_W: usize = 16;
const TOOL_W: usize = 10;
const DUR_W: usize = 6;
const INDENT: &str = "   ";
const WRAP_MAX: usize = 6;

#[derive(Debug)]
pub struct Drill {
    pub target: DrillTarget,
    offset: usize,
    pub q: Query<Box<DrillResult>>,
    sel: ListSel,
    expanded: HashSet<(String, u64)>,
    scroll: usize,
    follow: bool,
}

/// What a key asks of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrillOut {
    Close,
    Stay,
}

fn stamp(ts: Option<&str>) -> String {
    let Some(ts) = ts else {
        return "no timestamp".to_owned();
    };
    let b = ts.as_bytes();
    let digits =
        |r: std::ops::Range<usize>| b.get(r).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
    if digits(0..4)
        && b.get(4) == Some(&b'-')
        && digits(5..7)
        && b.get(7) == Some(&b'-')
        && digits(8..10)
        && b.get(10) == Some(&b'T')
        && digits(11..13)
        && b.get(13) == Some(&b':')
        && digits(14..16)
    {
        format!("{}-{} {}", &ts[5..7], &ts[8..10], &ts[11..16])
    } else {
        clip(&sanitize(ts), TS_W)
    }
}

fn sid_cell(sid: &str) -> String {
    let head = sanitize(short_id(sid));
    clip(
        if head.chars().count() >= 8 {
            &head
        } else {
            sid
        },
        SID_W,
    )
}

fn project_tail(p: &str) -> String {
    let n = p.chars().count();
    if n > PROJ_W {
        format!("…{}", p.chars().skip(n - (PROJ_W - 1)).collect::<String>())
    } else {
        p.to_owned()
    }
}

/// Hard-wrap one command, breaking on a late space; the last row is clipped.
fn wrapped(text: &str, w: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text.to_owned();
    while width(&rest) > w && out.len() < WRAP_MAX - 1 {
        let head = take_width(&rest, w);
        let cut = head
            .rfind(' ')
            .filter(|c| *c * 10 > w * 6)
            .unwrap_or(head.len());
        out.push(rest[..cut].to_owned());
        rest = peektrace_core::js::text::trim_start(&rest[cut..]).to_owned();
    }
    out.push(clip(&rest, w));
    out
}

fn evidence(line: &str, w: usize, expand: bool) -> Vec<String> {
    if !expand {
        return vec![format!("{INDENT}{}", first_line(line, w))];
    }
    sanitize(line)
        .split('\n')
        .flat_map(|one| wrapped(peektrace_core::js::text::trim(one), w))
        .take(WRAP_MAX)
        .map(|r| format!("{INDENT}{r}"))
        .collect()
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a page offset"
)]
const fn page_offset(o: f64) -> usize {
    o as usize
}

fn key_of(h: &DrillHit) -> (String, u64) {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a sequence number"
    )]
    let seq = h.sample.seq as u64;
    (h.sample.sid.clone(), seq)
}

impl Drill {
    pub fn open(
        target: DrillTarget,
        days: u32,
        agent: Option<peektrace_core::Agent>,
        cx: &mut Ctx<'_>,
    ) -> Self {
        let mut d = Self {
            target,
            offset: 0,
            q: Query::default(),
            sel: ListSel::default(),
            expanded: HashSet::new(),
            scroll: 0,
            follow: true,
        };
        d.fetch(days, agent, cx);
        d
    }

    fn fetch(&mut self, days: u32, agent: Option<peektrace_core::Agent>, cx: &mut Ctx<'_>) {
        let id = cx.request(Request::Drill {
            days,
            agent,
            table: self.target.table,
            key: self.target.key.clone(),
            offset: self.offset,
        });
        self.q.start(id);
    }

    fn hits(&self) -> &[DrillHit] {
        self.q.data.as_ref().map_or(&[][..], |d| d.hits.as_slice())
    }

    fn at(&self) -> usize {
        self.q
            .data
            .as_ref()
            .map_or(self.offset, |d| page_offset(d.offset))
    }

    fn total(&self) -> usize {
        self.q.data.as_ref().map_or(0, |d| d.total)
    }

    pub fn key(
        &mut self,
        k: KeyEvent,
        days: u32,
        agent: Option<peektrace_core::Agent>,
        cx: &mut Ctx<'_>,
    ) -> DrillOut {
        let n = self.hits().len();
        let (at, total) = (self.at(), self.total());
        match k.code {
            KeyCode::Esc | KeyCode::Left => return DrillOut::Close,
            KeyCode::Enter => {
                if let Some(h) = self.hits().get(self.sel.at(n)) {
                    let key = key_of(h);
                    if !self.expanded.remove(&key) {
                        self.expanded.insert(key);
                    }
                }
            }
            KeyCode::Char('n') if at + n < total => self.page(at + n, days, agent, cx),
            KeyCode::Char('p') if at > 0 => {
                self.page(at.saturating_sub(DRILL_PAGE), days, agent, cx);
            }
            code => {
                if self.sel.page(code, n, 10) || self.sel.key(code, n) {
                    self.follow = true;
                }
            }
        }
        DrillOut::Stay
    }

    fn page(
        &mut self,
        offset: usize,
        days: u32,
        agent: Option<peektrace_core::Agent>,
        cx: &mut Ctx<'_>,
    ) {
        self.offset = offset;
        self.sel.index = 0;
        self.scroll = 0;
        self.fetch(days, agent, cx);
    }

    fn hit_rows(
        &self,
        h: &DrillHit,
        selected: bool,
        line_max: usize,
        show_project: bool,
    ) -> Vec<Vec<Seg>> {
        let mut head = vec![
            seg(if selected { "▸ " } else { "  " }, fg(theme::PRIMARY)),
            seg(
                pad_end(&stamp(h.sample.ts.as_deref()), TS_W + 1),
                fg(theme::TEXT_FAINT),
            ),
            seg(
                pad_end(
                    &sid_cell(h.sample.parent.as_deref().unwrap_or(&h.sample.sid)),
                    SID_W + 1,
                ),
                fg(theme::TEXT_DIM),
            ),
        ];
        if show_project {
            head.push(seg(
                pad_end(&project_tail(&sanitize(&h.sample.project)), PROJ_W + 1),
                fg(theme::TEXT_FAINT),
            ));
        }
        head.push(seg(
            pad_end(&clip(&sanitize(&h.tool), TOOL_W), TOOL_W + 1),
            fg(theme::TEXT),
        ));
        head.push(seg(
            pad_start(&h.dur_sec.map_or_else(|| "—".to_owned(), hms), DUR_W),
            fg(theme::TEXT_DIM),
        ));
        head.push(if h.failed {
            seg("  fail", fg(theme::BAD))
        } else {
            seg("  —", fg(theme::TEXT_FAINT))
        });
        let expand = self.expanded.contains(&key_of(h));
        let mut rows = vec![head];
        let style = fg(if selected {
            theme::TEXT
        } else {
            theme::TEXT_FAINT
        });
        rows.extend(
            evidence(&h.sample.line, line_max, expand)
                .into_iter()
                .map(|l| vec![seg(l, style)]),
        );
        if let Some(pair) = &h.pair_line {
            let style = fg(if selected {
                theme::GOOD
            } else {
                theme::TEXT_FAINT
            });
            rows.extend(
                evidence(&format!("→ {pair}"), line_max, expand)
                    .into_iter()
                    .map(|l| vec![seg(l, style)]),
            );
        }
        rows
    }

    #[expect(clippy::too_many_lines, reason = "head, list and footer of one panel")]
    pub fn draw(&mut self, buf: &mut Buffer, area: Rect, content: usize) {
        let inner_w = content.saturating_sub(4).max(40);
        let title = clip(
            &format!(
                "DRILL  --{} {}",
                self.target.table.as_str(),
                sanitize(&self.target.key)
            ),
            inner_w,
        );
        let inner = panel(buf, area, &title, true, 1);
        if inner.height == 0 {
            return;
        }
        let mut top: Vec<Vec<Seg>> = vec![vec![seg(
            clip(&sanitize(&self.target.label), inner_w),
            fg(theme::TEXT),
        )]];
        if let Some(n) = &self.target.note {
            top.push(vec![seg(clip(&sanitize(n), inner_w), fg(theme::TEXT_DIM))]);
        }
        top.push(vec![seg(
            clip(&sanitize(&self.target.spread), inner_w),
            fg(theme::TEXT_FAINT),
        )]);
        let count = self.hits().len();
        if self.q.loading() && count == 0 {
            top.push(loading("Reading the calls behind this row…"));
        }
        if let Some(e) = &self.q.error {
            top.push(error_line(e));
        }
        if !self.q.loading() && self.q.error.is_none() && count == 0 {
            top.push(empty("No calls behind this row."));
        }
        let mut y = inner.y;
        for row in &top {
            if y < inner.bottom() {
                put(buf, inner.x, y, inner.width, row);
            }
            y += 1;
        }
        let foot_y = inner.bottom().saturating_sub(3);
        let (at, total) = (self.at(), self.total());
        let sel = self.sel.at(count);
        let position = if total == 0 {
            "no calls".to_owned()
        } else {
            #[expect(clippy::cast_precision_loss, reason = "hit counts")]
            let (a, b, t) = ((at + 1) as f64, (at + count) as f64, total as f64);
            format!("hits {}–{} of {} calls", fmt(a), fmt(b), fmt(t))
        };
        let route = self.hits().get(sel).map_or_else(String::new, |h| {
            format!(
                "open it: {}",
                sanitize(&open_command(&h.sample.sid, h.sample.parent.as_deref()))
            )
        });
        let mut hints: Vec<(&str, &str)> = vec![("↑↓/jk", "move"), ("⏎", "full line")];
        if at + count < total {
            hints.push(("n", "next page"));
        }
        if at > 0 {
            hints.push(("p", "prev page"));
        }
        hints.push(("esc", "close"));
        put(
            buf,
            inner.x,
            foot_y,
            inner.width,
            &[seg(position, fg(theme::TEXT_DIM))],
        );
        put(
            buf,
            inner.x,
            foot_y + 1,
            inner.width,
            &[seg(clip(&route, inner_w), fg(theme::TEXT_FAINT))],
        );
        put(buf, inner.x, foot_y + 2, inner.width, &key_hints(&hints));
        if count == 0 || y + 1 >= foot_y {
            return;
        }
        let list = Rect::new(inner.x, y + 1, inner.width, foot_y - y - 1);
        let line_max = inner_w.saturating_sub(INDENT.len() + 1);
        let show_project = content >= 96;
        let blocks: Vec<Vec<Vec<Seg>>> = self
            .hits()
            .iter()
            .enumerate()
            .map(|(i, h)| self.hit_rows(h, i == sel, line_max, show_project))
            .collect();
        let tops: Vec<usize> = blocks
            .iter()
            .scan(0, |acc, b| {
                let t = *acc;
                *acc += b.len();
                Some(t)
            })
            .collect();
        let view = usize::from(list.height);
        if self.follow {
            let (t, b) = (tops[sel], tops[sel] + blocks[sel].len());
            if t < self.scroll {
                self.scroll = t;
            } else if b > self.scroll + view {
                self.scroll = (b - view).min(t);
            }
            self.follow = false;
        }
        for (i, block) in blocks.iter().enumerate() {
            for (j, row) in block.iter().enumerate() {
                let at = tops[i] + j;
                if at < self.scroll || at >= self.scroll + view {
                    continue;
                }
                let yy = list.y + u16::try_from(at - self.scroll).unwrap_or(0);
                if i == sel {
                    fill_row(
                        buf,
                        list.x,
                        yy,
                        list.width,
                        Style::new().bg(theme::PANEL_SEL),
                    );
                }
                put(buf, list.x, yy, list.width, row);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_and_tails() {
        assert_eq!(stamp(Some("2026-11-11T16:42:21.851Z")), "11-11 16:42");
        assert_eq!(stamp(None), "no timestamp");
        assert_eq!(
            project_tail("/a/very/long/project/path"),
            "…ng/project/path"
        );
        assert_eq!(
            wrapped("aaaa bbbb cccc", 9),
            vec!["aaaa bbbb".to_owned(), "cccc".to_owned()]
        );
    }

    #[test]
    fn evidence_strips_escapes() {
        let rows = evidence("\u{1b}[31mrm -rf x\u{1b}[0m", 40, false);
        assert_eq!(rows, vec!["   rm -rf x".to_owned()]);
        assert!(!rows.concat().contains('\u{1b}'));
    }
}
