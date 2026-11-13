//! Memory: every Claude vault as a flat list, with the selected vault's
//! budget and index diff, or the selected entry's metadata and body.

mod panes;
pub mod rows;

use peektrace_core::Agent;
use peektrace_core::capabilities::{MEMORY_CRUD, supports};
use peektrace_core::memory::read::AllVaults;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;

use self::rows::{Row, build_rows, next_type};
use super::{Ctx, Query};
use crate::backend::{Reply, Request, Response};
use crate::draw::{fill_row, panel, put, put_lr, seg};
use crate::fmt::{clip, fmt_bytes};
use crate::input::{Edit, LineInput};
use crate::list::{ListSel, window_slice};
use crate::theme::{self, fg};
use crate::widgets::{empty, error_line, key_hints, loading};

const DETAIL_W: u16 = 52;
const TITLE_W: usize = 26;

#[derive(Debug, Default)]
pub struct State {
    vaults: Query<Box<AllVaults>>,
    ver: Option<u64>,
    search: LineInput,
    pub searching: bool,
    ty: &'static str,
    pub body_focused: bool,
    sel: ListSel,
    body_scroll: usize,
}

impl State {
    pub const fn typing(&self) -> bool {
        self.searching
    }

    fn rows(&self) -> Vec<Row> {
        let vaults = self
            .vaults
            .data
            .as_ref()
            .map_or(&[][..], |a| a.vaults.as_slice());
        build_rows(vaults, &self.search.text.to_lowercase(), self.ty())
    }

    fn ty(&self) -> &'static str {
        if self.ty.is_empty() { "all" } else { self.ty }
    }

    fn key_of(&self, row: Row) -> Option<(String, Option<String>)> {
        let a = self.vaults.data.as_ref()?;
        Some(match row {
            Row::Header { vault, .. } => (a.vaults[vault].slug.clone(), None),
            Row::Entry { vault, entry } => (
                a.vaults[vault].slug.clone(),
                Some(a.vaults[vault].entries[entry].slug.clone()),
            ),
        })
    }

    fn fetch(&mut self, cx: &mut Ctx<'_>) {
        self.ver = Some(cx.watch.memory);
        let id = cx.request(Request::Vaults);
        self.vaults.start(id);
    }

    pub fn enter(&mut self, cx: &mut Ctx<'_>) {
        if self.ver != Some(cx.watch.memory) && !self.vaults.loading() {
            self.fetch(cx);
        }
    }

    pub fn watched(&mut self, cx: &mut Ctx<'_>, active: bool) {
        if active {
            self.fetch(cx);
        }
    }

    pub fn reply(&mut self, r: Reply) {
        if let Response::Vaults(res) = r.resp {
            let rows = self.rows();
            let keep = rows
                .get(self.sel.at(rows.len()))
                .and_then(|row| self.key_of(*row));
            if self.vaults.land(r.id, res)
                && let Some(keep) = keep
            {
                let rows = self.rows();
                if let Some(at) = rows
                    .iter()
                    .position(|row| self.key_of(*row).as_ref() == Some(&keep))
                {
                    self.sel.index = at;
                }
            }
            self.settle_focus();
        }
    }

    /// Only an entry has a body to focus; any other selection hands the
    /// keys back to the list.
    fn settle_focus(&mut self) {
        let rows = self.rows();
        if !matches!(rows.get(self.sel.at(rows.len())), Some(Row::Entry { .. })) {
            self.body_focused = false;
        }
    }

    pub fn paste(&mut self, s: &str) {
        if self.searching {
            self.search.insert(s);
            self.sel.clamp(self.rows().len());
        }
    }

    pub fn key(&mut self, k: KeyEvent) {
        if self.searching {
            if matches!(self.search.key(k), Edit::Submit | Edit::Cancel) {
                self.searching = false;
            }
            self.sel.clamp(self.rows().len());
            self.settle_focus();
            return;
        }
        let rows = self.rows();
        let selected = rows.get(self.sel.at(rows.len())).copied();
        match k.code {
            KeyCode::Char('/') => {
                self.searching = true;
                self.body_focused = false;
            }
            KeyCode::Char('t') => {
                self.ty = next_type(self.ty());
                self.sel.index = 0;
            }
            KeyCode::Tab if matches!(selected, Some(Row::Entry { .. })) => {
                self.body_focused = !self.body_focused;
            }
            code if self.body_focused => match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.body_scroll = self.body_scroll.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => self.body_scroll += 1,
                KeyCode::Home => self.body_scroll = 0,
                KeyCode::End => self.body_scroll = usize::MAX,
                _ => {}
            },
            code => {
                let before = self.sel.at(rows.len());
                self.sel.key(code, rows.len());
                if self.sel.at(rows.len()) != before {
                    self.body_scroll = 0;
                }
            }
        }
        self.settle_focus();
    }

    pub fn draw(&mut self, buf: &mut Buffer, area: Rect) {
        if area.height < 4 {
            return;
        }
        let mut top = vec![seg("/", fg(theme::ACCENT)), seg(" ", Style::new())];
        let type_label = format!(" type:{}", self.ty());
        let room = usize::from(area.width).saturating_sub(type_label.len() + 2);
        if self.searching {
            top.extend(
                self.search
                    .view("filter slug · name · description · body", room),
            );
        } else if self.search.text.is_empty() {
            top.push(seg("search (press /)", fg(theme::TEXT_FAINT)));
        } else {
            top.push(seg(clip(&self.search.text, room), fg(theme::TEXT)));
        }
        top.push(seg(type_label, fg(theme::TEXT_DIM)));
        put(buf, area.x, area.y, area.width, &top);
        let main = Rect::new(area.x, area.y + 1, area.width, area.height - 3);
        let rows = self.rows();
        self.sel.clamp(rows.len());
        let index = self.sel.index;
        let selected = rows.get(index).copied();
        let detail = main.width > DETAIL_W + 24 && selected.is_some();
        let list_w = if detail {
            main.width - DETAIL_W - 1
        } else {
            main.width
        };
        let inner = panel(
            buf,
            Rect::new(main.x, main.y, list_w, main.height),
            &format!("Memory ({})", rows.len()),
            !self.body_focused,
            1,
        );
        self.draw_list(buf, inner, &rows, index, area.height + 2);
        if detail && let (Some(row), Some(all)) = (selected, self.vaults.data.as_deref()) {
            let r = Rect::new(main.right() - DETAIL_W, main.y, DETAIL_W, main.height);
            match row {
                Row::Header { vault, .. } => panes::vault(buf, r, &all.vaults[vault]),
                Row::Entry { vault, entry } => {
                    let e = &all.vaults[vault].entries[entry];
                    self.body_scroll = panes::entry(buf, r, e, self.body_focused, self.body_scroll);
                }
            }
        }
        let crud = if supports(MEMORY_CRUD, Agent::Claude) {
            "supported"
        } else {
            "read-only"
        };
        put(
            buf,
            area.x,
            area.bottom() - 2,
            area.width,
            &[seg(
                format!("editing available in the web app (peektrace serve) · memory.crud: {crud}"),
                fg(theme::TEXT_FAINT),
            )],
        );
        let hints = key_hints(&[
            ("↑↓/jk", "move"),
            ("/", "search"),
            ("t", "type"),
            ("tab", if self.body_focused { "list" } else { "body" }),
            ("q", "quit"),
        ]);
        put(buf, area.x, area.bottom() - 1, area.width, &hints);
    }

    #[expect(clippy::too_many_lines, reason = "status lines, then one row per kind")]
    fn draw_list(&self, buf: &mut Buffer, inner: Rect, rows: &[Row], index: usize, term_h: u16) {
        let mut y = inner.y;
        let line = |buf: &mut Buffer, y: &mut u16, segs: &[crate::draw::Seg]| {
            if *y < inner.bottom() {
                put(buf, inner.x, *y, inner.width, segs);
            }
            *y += 1;
        };
        let all = self.vaults.data.as_deref();
        if self.vaults.loading() && all.is_none() {
            line(buf, &mut y, &loading("Loading…"));
        }
        if let Some(e) = &self.vaults.error {
            line(buf, &mut y, &error_line(e));
        }
        let no_projects = all.is_some_and(|a| a.projects.is_empty());
        if no_projects {
            line(buf, &mut y, &empty("No memories found."));
        }
        if !self.vaults.loading() && self.vaults.error.is_none() && !no_projects && rows.is_empty()
        {
            line(
                buf,
                &mut y,
                &empty("No memories match the current filters."),
            );
        }
        let Some(all) = all else { return };
        let visible = usize::from(term_h).saturating_sub(11).max(4);
        let (start, end) = window_slice(index, rows.len(), visible);
        for (abs, row) in rows.iter().enumerate().take(end).skip(start) {
            if y >= inner.bottom() {
                break;
            }
            let selected = abs == index;
            if selected {
                fill_row(
                    buf,
                    inner.x,
                    y,
                    inner.width,
                    Style::new().bg(theme::PANEL_SEL),
                );
            }
            match *row {
                Row::Header { vault, matched } => {
                    let v = &all.vaults[vault];
                    let mut segs = vec![
                        seg(
                            format!(
                                "{}{}",
                                if selected { "▸ " } else { "  " },
                                clip(&v.project, TITLE_W)
                            ),
                            fg(if selected {
                                theme::PRIMARY
                            } else {
                                theme::ACCENT
                            }),
                        ),
                        seg(format!(" ({matched})"), fg(theme::TEXT_FAINT)),
                    ];
                    if v.budget.over_budget {
                        segs.push(seg(" [over budget]", fg(theme::BAD)));
                    }
                    put(buf, inner.x, y, inner.width, &segs);
                }
                Row::Entry { vault, entry } => {
                    let e = &all.vaults[vault].entries[entry];
                    let name = e.name().unwrap_or(&e.slug);
                    let left = vec![
                        seg(
                            format!(
                                "{}{}",
                                if selected { "  › " } else { "    " },
                                clip(name, TITLE_W)
                            ),
                            fg(if selected {
                                theme::PRIMARY
                            } else {
                                theme::TEXT
                            }),
                        ),
                        match e.kind() {
                            None => seg(" —", fg(theme::TEXT_FAINT)),
                            Some(t) => seg(format!(" [{t}]"), fg(theme::TEXT_DIM)),
                        },
                    ];
                    let right = vec![seg(
                        format!(
                            " {} {} ↗{}",
                            fmt_bytes(u64::try_from(e.size).unwrap_or(u64::MAX)),
                            if e.in_index { "✓" } else { "✗" },
                            e.links.len()
                        ),
                        fg(theme::TEXT_DIM),
                    )];
                    put_lr(buf, inner.x, y, inner.width, &left, &right);
                }
            }
            y += 1;
        }
    }
}

#[cfg(test)]
mod tests;
