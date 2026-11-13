//! Settings: edit the extra agent roots in `settings.json`. Edits stay
//! local until a save, which is a compare-and-swap on the file's mtime.

pub mod rows;

use peektrace_core::Agent;
use peektrace_core::settings_doc::StoredSettings;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;

use self::rows::{EditRow, Nav, Rows, agent_at, assemble, build_entries, seed_rows};
use super::{Ctx, Query};
use crate::backend::{Reply, Request, Response, SaveError};
use crate::draw::{Seg, frame, inset, panel, put, seg};
use crate::fmt::{clip, pad_end};
use crate::input::{Edit, LineInput, ctrl};
use crate::list::{ListSel, window_slice};
use crate::theme::{self, agent_label, fg};
use crate::widgets::{key_hints, loading};

const PATH_W: usize = 38;
const LABEL_W: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Path,
    Label,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Idle,
    Saving,
    Saved,
    Conflict,
    Error(String),
}

#[derive(Debug, Default)]
pub struct State {
    q: Query<StoredSettings>,
    loaded: bool,
    seeded: Option<i64>,
    rows: Rows,
    next_id: u64,
    mtime: Option<i64>,
    editing: Option<(Agent, u64, Field)>,
    draft: LineInput,
    pub status: Status,
    sel: ListSel,
    save: Option<u64>,
}

impl State {
    pub const fn typing(&self) -> bool {
        self.editing.is_some()
    }

    pub fn enter(&mut self, cx: &mut Ctx<'_>) {
        if !self.loaded {
            self.loaded = true;
            self.fetch(cx);
        }
    }

    fn fetch(&mut self, cx: &mut Ctx<'_>) {
        let id = cx.request(Request::SettingsGet);
        self.q.start(id);
    }

    fn reseed(&mut self, s: &StoredSettings) {
        self.seeded = Some(s.mtime_ms);
        self.mtime = Some(s.mtime_ms);
        self.rows = seed_rows(&s.doc, &mut self.next_id);
        self.editing = None;
    }

    pub fn reply(&mut self, r: Reply) {
        match r.resp {
            Response::Settings(res) => {
                if self.q.land(r.id, res)
                    && let Some(s) = self.q.data.clone()
                    && self.seeded != Some(s.mtime_ms)
                {
                    self.reseed(&s);
                    self.status = Status::Idle;
                }
            }
            Response::Saved(res) if self.save == Some(r.id) => {
                self.save = None;
                self.status = match res {
                    Ok(s) => {
                        self.reseed(&s);
                        Status::Saved
                    }
                    Err(SaveError::Changed) => Status::Conflict,
                    Err(SaveError::Other(m)) => Status::Error(m),
                };
            }
            _ => {}
        }
    }

    fn row_mut(&mut self, agent: Agent, id: u64) -> Option<&mut EditRow> {
        self.rows[agent_at(agent)].iter_mut().find(|r| r.id == id)
    }

    fn value(&self, agent: Agent, id: u64, f: Field) -> String {
        self.rows[agent_at(agent)]
            .iter()
            .find(|r| r.id == id)
            .map(|r| match f {
                Field::Path => r.path.clone(),
                Field::Label => r.label.clone(),
            })
            .unwrap_or_default()
    }

    fn start_edit(&mut self, agent: Agent, id: u64, f: Field) {
        self.draft.set(&self.value(agent, id, f));
        self.editing = Some((agent, id, f));
    }

    fn commit(&mut self) {
        if let Some((agent, id, f)) = self.editing {
            let v = self.draft.text.clone();
            if let Some(r) = self.row_mut(agent, id) {
                match f {
                    Field::Path => r.path = v,
                    Field::Label => r.label = v,
                }
            }
        }
    }

    fn add_row(&mut self, agent: Agent) {
        self.next_id += 1;
        let id = self.next_id;
        self.rows[agent_at(agent)].push(EditRow {
            id,
            path: String::new(),
            label: String::new(),
        });
        let at = build_entries(&self.rows)
            .iter()
            .position(|n| *n == Nav::Root { agent, id });
        if let Some(at) = at {
            self.sel.index = at;
        }
        self.draft.set("");
        self.editing = Some((agent, id, Field::Path));
    }

    fn send(&mut self, cx: &mut Ctx<'_>, guard: bool) {
        self.status = Status::Saving;
        #[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
        let expected = if guard {
            self.mtime.map(|m| m as f64)
        } else {
            None
        };
        let id = cx.request(Request::SettingsSave {
            doc: assemble(&self.rows),
            expected,
        });
        self.save = Some(id);
    }

    pub fn paste(&mut self, s: &str) {
        if self.editing.is_some() {
            self.draft.insert(s);
        }
    }

    pub fn key(&mut self, k: KeyEvent, cx: &mut Ctx<'_>) {
        if let Some((agent, id, f)) = self.editing {
            match k.code {
                KeyCode::Esc => self.editing = None,
                KeyCode::Tab => {
                    self.commit();
                    let next = if f == Field::Path {
                        Field::Label
                    } else {
                        Field::Path
                    };
                    self.start_edit(agent, id, next);
                }
                _ => {
                    if self.draft.key(k) == Edit::Submit {
                        self.commit();
                        if f == Field::Path {
                            self.start_edit(agent, id, Field::Label);
                        } else {
                            self.editing = None;
                        }
                    }
                }
            }
            return;
        }
        if self.status == Status::Conflict {
            match k.code {
                KeyCode::Char('r') => {
                    self.status = Status::Idle;
                    self.fetch(cx);
                    return;
                }
                KeyCode::Char('o') => {
                    self.send(cx, false);
                    return;
                }
                _ => {}
            }
        }
        if ctrl(&k) && k.code == KeyCode::Char('s') {
            self.send(cx, true);
            return;
        }
        let entries = build_entries(&self.rows);
        if self.sel.key(k.code, entries.len()) {
            return;
        }
        let Some(active) = entries.get(self.sel.at(entries.len())).copied() else {
            return;
        };
        match (k.code, active) {
            (KeyCode::Enter | KeyCode::Char('e'), Nav::Add { agent })
            | (KeyCode::Char('a'), Nav::Add { agent } | Nav::Root { agent, .. }) => {
                self.add_row(agent);
            }
            (KeyCode::Enter | KeyCode::Char('e'), Nav::Root { agent, id }) => {
                self.start_edit(agent, id, Field::Path);
            }
            (
                KeyCode::Char('d') | KeyCode::Delete | KeyCode::Backspace,
                Nav::Root { agent, id },
            ) => {
                self.rows[agent_at(agent)].retain(|r| r.id != id);
            }
            (KeyCode::Char('s'), _) => self.send(cx, true),
            _ => {}
        }
    }

    pub fn draw(&self, buf: &mut Buffer, area: Rect) {
        if area.height < 6 {
            return;
        }
        let warn = fg(theme::WARN);
        let head = Rect::new(area.x, area.y, area.width, 4);
        let inner = inset(frame(buf, head, Some("settings.json"), warn, warn), 1, 0);
        put(
            buf,
            inner.x,
            inner.y,
            inner.width,
            &[seg(
                "Changes apply after a restart of `peektrace serve`",
                warn,
            )],
        );
        let path = self.q.data.as_ref().map_or("…", |d| d.path.as_str());
        put(
            buf,
            inner.x,
            inner.y + 1,
            inner.width,
            &[seg(path, fg(theme::TEXT_FAINT))],
        );
        let status = match &self.status {
            Status::Saving => seg("Saving…", fg(theme::TEXT_DIM)),
            Status::Saved => seg(
                "Saved — restart `peektrace serve` to apply.",
                fg(theme::GOOD),
            ),
            Status::Error(m) => seg(format!("✗ {}", clip(m, 160)), fg(theme::BAD)),
            Status::Idle | Status::Conflict => seg(" ", fg(theme::TEXT_FAINT)),
        };
        put(buf, area.x, area.y + 4, area.width, &[status]);
        let banner = if self.status == Status::Conflict {
            4
        } else {
            0
        };
        let list_h = area.height.saturating_sub(5 + 1 + banner);
        let list = panel(
            buf,
            Rect::new(area.x, area.y + 5, area.width, list_h),
            "Agent roots",
            true,
            1,
        );
        self.draw_rows(buf, list);
        if banner > 0 {
            let b = Rect::new(area.x, area.y + 5 + list_h, area.width, banner);
            let inner = inset(frame(buf, b, None, warn, warn), 1, 0);
            put(
                buf,
                inner.x,
                inner.y,
                inner.width,
                &[seg("settings.json changed on disk.", warn)],
            );
            let keys = [
                seg("r", fg(theme::ACCENT)),
                seg("  reload (discard edits)  ", fg(theme::TEXT_FAINT)),
                seg("o", fg(theme::ACCENT)),
                seg("  overwrite", fg(theme::TEXT_FAINT)),
            ];
            put(buf, inner.x, inner.y + 1, inner.width, &keys);
        }
        let hints = key_hints(&[
            ("↑↓/jk", "move"),
            ("e/↵", "edit"),
            ("a", "add"),
            ("d", "remove"),
            ("^s", "save"),
            ("1-5", "section"),
            ("q", "quit"),
        ]);
        put(buf, area.x, area.bottom() - 1, area.width, &hints);
    }

    fn cell(
        &self,
        value: &str,
        placeholder: &str,
        w: usize,
        editing: bool,
        selected: bool,
    ) -> Vec<Seg> {
        if editing {
            let mut segs = self.draft.view(placeholder, w);
            let used = crate::draw::segs_width(&segs);
            segs.push(seg(" ".repeat(w.saturating_sub(used)), Style::new()));
            return segs;
        }
        let style = if value.is_empty() {
            fg(theme::TEXT_FAINT)
        } else if selected {
            fg(theme::TEXT)
        } else {
            fg(theme::TEXT_DIM)
        };
        let shown = if value.is_empty() {
            placeholder.to_owned()
        } else {
            clip(value, w - 1)
        };
        vec![seg(pad_end(&shown, w), style)]
    }

    fn draw_rows(&self, buf: &mut Buffer, inner: Rect) {
        let entries = build_entries(&self.rows);
        let active = entries.get(self.sel.at(entries.len())).copied();
        let mut lines: Vec<(Vec<Seg>, bool)> = Vec::new();
        if self.q.loading() && self.q.data.is_none() {
            lines.push((loading("Loading…"), false));
        }
        for (agent, list) in Agent::ALL.iter().zip(&self.rows) {
            lines.push((vec![seg(agent_label(*agent), fg(theme::ACCENT))], false));
            if list.is_empty() {
                lines.push((vec![seg("  No extra roots.", fg(theme::TEXT_FAINT))], false));
            }
            for r in list {
                let selected = active
                    == Some(Nav::Root {
                        agent: *agent,
                        id: r.id,
                    });
                let ed = |f| self.editing == Some((*agent, r.id, f));
                let mut row = vec![
                    seg(
                        if selected { "›" } else { " " },
                        fg(if selected {
                            theme::PRIMARY
                        } else {
                            theme::TEXT_FAINT
                        }),
                    ),
                    seg(" ", Style::new()),
                ];
                row.extend(self.cell(&r.path, "~/work/.claude", PATH_W, ed(Field::Path), selected));
                row.push(seg(" ", Style::new()));
                row.extend(self.cell(&r.label, "label", LABEL_W, ed(Field::Label), selected));
                if selected && self.editing.is_none() {
                    row.push(seg(" d:del", fg(theme::TEXT_FAINT)));
                }
                lines.push((row, selected));
            }
            let add = active == Some(Nav::Add { agent: *agent });
            lines.push((
                vec![seg(
                    format!("{}+ add root", if add { "› " } else { "  " }),
                    fg(if add {
                        theme::PRIMARY
                    } else {
                        theme::TEXT_FAINT
                    }),
                )],
                add,
            ));
        }
        let at = lines.iter().position(|(_, s)| *s).unwrap_or(0);
        let (start, end) = window_slice(at, lines.len(), usize::from(inner.height));
        for (i, (row, _)) in lines[start..end].iter().enumerate() {
            put(
                buf,
                inner.x,
                inner.y + u16::try_from(i).unwrap_or(0),
                inner.width,
                row,
            );
        }
    }
}
