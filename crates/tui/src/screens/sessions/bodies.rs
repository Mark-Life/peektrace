//! Expanded history bodies: decoded, sanitized and capped once per event,
//! measured per width by a plain wrap count, and highlighted only when a
//! row is on screen. Entries outlive a re-analysis while their event stays
//! the same, since a live transcript only grows.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};

use peektrace_core::highlight::Lang;
use peektrace_core::sessions::labels::context_edit_note;
use peektrace_core::sessions::model::{AnalyzedSession, EventKind, TimelineEvent};
use rayon::prelude::*;

use super::decode::decode_body;
use crate::draw::{Seg, wrap_count, wrap_text};
use crate::fmt::{clip, width};
use crate::syntax::{capped, paint, shown_text};
use crate::theme::{self, fg};

const BODY_MAX_LINES: usize = 120;
const EDIT_NOTE_MAX: usize = 600;
/// Bytes hashed at each end of a body to tell a changed event apart.
const ENDS: usize = 256;

/// What identifies an event's body across analyses.
#[derive(Debug, PartialEq, Eq)]
struct Key {
    kind: EventKind,
    len: usize,
    ends: u64,
    edit: Option<String>,
}

impl Key {
    fn of(e: &TimelineEvent) -> Self {
        let b = e.body.as_bytes();
        let mut h = DefaultHasher::new();
        b[..b.len().min(ENDS)].hash(&mut h);
        b[b.len().saturating_sub(ENDS)..].hash(&mut h);
        Self {
            kind: e.kind,
            len: b.len(),
            ends: h.finish(),
            edit: context_edit_note(e),
        }
    }
}

#[derive(Debug)]
enum Shown {
    Note(String),
    Code { text: String, lang: Option<Lang> },
}

impl Shown {
    /// The text that gets wrapped.
    fn text(&self) -> &str {
        match self {
            Self::Note(n) => n,
            Self::Code { text, .. } => shown_text(text),
        }
    }
}

/// Rows `text` wraps to at `w` columns, from its line widths: a line that
/// fits takes one row and is not walked again.
fn wrapped_rows(text: &str, widths: &[usize], w: usize) -> usize {
    text.split('\n')
        .zip(widths)
        .map(|(line, lw)| {
            if *lw <= w.max(1) {
                1
            } else {
                wrap_count(line, w)
            }
        })
        .sum()
}

fn line_widths(text: &str) -> Vec<usize> {
    text.split('\n').map(width).collect()
}

#[derive(Debug)]
struct Body {
    key: Key,
    edit: Option<String>,
    shown: Shown,
    /// Display width of each line of the shown text.
    widths: Vec<usize>,
    height: Option<(u16, usize)>,
    rows: Option<(u16, Vec<Vec<Seg>>)>,
}

impl Body {
    fn new(e: &TimelineEvent) -> Self {
        let key = Key::of(e);
        let edit = key.edit.as_deref().map(|n| clip(n, EDIT_NOTE_MAX));
        let d = decode_body(e);
        let shown = match d.note {
            Some(note) => Shown::Note(note),
            None => Shown::Code {
                text: capped(&d.content, BODY_MAX_LINES),
                lang: d.lang,
            },
        };
        let widths = line_widths(shown.text());
        Self {
            key,
            edit,
            shown,
            widths,
            height: None,
            rows: None,
        }
    }

    fn height(&mut self, w: u16) -> usize {
        if let Some((hw, h)) = self.height
            && hw == w
        {
            return h;
        }
        let cols = usize::from(w);
        let edit = self.edit.as_deref().map_or(0, |n| wrap_count(n, cols));
        let body = wrapped_rows(self.shown.text(), &self.widths, cols);
        self.height = Some((w, edit + body));
        edit + body
    }

    fn rows(&mut self, w: u16) -> &[Vec<Seg>] {
        if self.rows.as_ref().is_none_or(|(rw, _)| *rw != w) {
            let cols = usize::from(w);
            let mut rows = self
                .edit
                .as_deref()
                .map(|n| wrap_text(n, cols, fg(theme::WARN)))
                .unwrap_or_default();
            match &self.shown {
                Shown::Note(n) => rows.extend(wrap_text(n, cols, fg(theme::TEXT_FAINT))),
                Shown::Code { text, lang } => rows.extend(paint(text, *lang, cols)),
            }
            self.height = Some((w, rows.len()));
            self.rows = Some((w, rows));
        }
        self.rows.as_ref().map_or(&[], |(_, r)| r.as_slice())
    }
}

/// Bodies by position in `events`.
#[derive(Debug, Default)]
pub struct Bodies {
    entries: HashMap<usize, Body>,
    generation: Option<u64>,
}

impl Bodies {
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// On a new analysis, drop the entries whose event changed.
    pub fn sync(&mut self, s: &AnalyzedSession, generation: u64) {
        if self.generation == Some(generation) {
            return;
        }
        self.generation = Some(generation);
        let events = &s.parsed.events;
        self.entries
            .retain(|id, b| events.get(*id).is_some_and(|e| Key::of(e) == b.key));
    }

    /// Prepare the bodies of `ids` not prepared yet, in parallel.
    pub fn prepare(&mut self, s: &AnalyzedSession, ids: impl Iterator<Item = usize>) {
        let missing: Vec<usize> = ids.filter(|id| !self.entries.contains_key(id)).collect();
        if missing.is_empty() {
            return;
        }
        let made: Vec<(usize, Body)> = missing
            .into_par_iter()
            .map(|id| (id, Body::new(&s.parsed.events[id])))
            .collect();
        self.entries.extend(made);
    }

    fn entry(&mut self, s: &AnalyzedSession, id: usize) -> &mut Body {
        self.entries
            .entry(id)
            .or_insert_with(|| Body::new(&s.parsed.events[id]))
    }

    /// Rows the body of event `id` takes at `w` columns.
    pub fn height(&mut self, s: &AnalyzedSession, id: usize, w: u16) -> usize {
        self.entry(s, id).height(w)
    }

    /// The body of event `id`, highlighted and wrapped to `w` columns.
    pub fn rows(&mut self, s: &AnalyzedSession, id: usize, w: u16) -> &[Vec<Seg>] {
        self.entry(s, id).rows(w)
    }
}

#[cfg(test)]
mod tests {
    use peektrace_core::sessions::model::EventShape;

    use super::*;

    fn event(index: usize, body: &str) -> TimelineEvent {
        TimelineEvent::new(EventShape::Prompt, index, EventKind::AssistantText).text(
            "t".to_owned(),
            body.to_owned(),
            body.to_owned(),
            1.0,
        )
    }

    #[test]
    fn a_new_analysis_keeps_the_bodies_of_unchanged_events() {
        let mut s = AnalyzedSession::default();
        s.parsed.events = vec![event(0, "one"), event(1, "two")];
        let mut b = Bodies::default();
        b.sync(&s, 1);
        b.prepare(&s, 0..2);
        assert_eq!(b.height(&s, 0, 20), 1);
        s.parsed.events[1].body.push_str(" and more");
        s.parsed.events.push(event(2, "three"));
        b.sync(&s, 2);
        assert!(b.entries.contains_key(&0), "unchanged event kept");
        assert!(!b.entries.contains_key(&1), "grown event dropped");
        let text: String = b.rows(&s, 1, 20)[0]
            .iter()
            .map(|(t, _)| t.as_str())
            .collect();
        assert_eq!(text, "two and more");
    }

    #[test]
    fn heights_follow_the_width() {
        let s = AnalyzedSession {
            parsed: peektrace_core::sessions::model::ParsedSession {
                events: vec![event(0, "aaaa bbbb cccc\nshort")],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut b = Bodies::default();
        assert_eq!(b.height(&s, 0, 40), 2);
        assert_eq!(b.height(&s, 0, 5), 4);
        assert_eq!(b.rows(&s, 0, 5).len(), 4);
    }
}
