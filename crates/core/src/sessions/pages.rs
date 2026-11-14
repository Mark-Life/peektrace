//! One analysis cut into the parts the session detail loads on its own: a
//! summary, the growth series, a body-free history index, event pages and
//! history search. Each part carries hashes, so a client refetches only what
//! changed.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::js::json_out::Writer;
use crate::js::{text, units};
use crate::sessions::model::{AnalyzedSession, BudgetKey, ParsedSession, TurnShape};
use crate::sessions::model::{TimelineEvent, Turn, TurnField};
use crate::sessions::write::{write_analysis_fields, write_event_fields};

/// Events per page the client asks for.
pub const PAGE_SIZE: usize = 100;
/// Most events one page request may ask for.
pub const MAX_LIMIT: usize = 500;

/// Growth slice columns, floor to top (the chart's stack order).
const STACK: [BudgetKey; 10] = [
    BudgetKey::SystemTools,
    BudgetKey::Listings,
    BudgetKey::Memory,
    BudgetKey::Files,
    BudgetKey::Prompts,
    BudgetKey::ToolResults,
    BudgetKey::AssistantText,
    BudgetKey::Thinking,
    BudgetKey::Other,
    BudgetKey::Unattributed,
];

/// FNV-1a 64 over bytes, fed in pieces.
#[derive(Debug, Clone, Copy)]
struct Fnv(u64);

impl Fnv {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn write(mut self, bytes: &[u8]) -> Self {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
        self
    }

    fn hex(self) -> String {
        format!("{:016x}", self.0)
    }
}

/// The hash of `text`: 16 lowercase hex chars.
pub fn hash_hex(text: &str) -> String {
    Fnv::new().write(text.as_bytes()).hex()
}

/// The hash of the JSON array `[items...]`, without building it.
fn array_hash(items: &[String]) -> String {
    let h = items
        .iter()
        .enumerate()
        .fold(Fnv::new().write(b"["), |h, (i, s)| {
            (if i == 0 { h } else { h.write(b",") }).write(s.as_bytes())
        });
    h.write(b"]").hex()
}

fn push_array(out: &mut String, items: &[String]) {
    out.push('[');
    for (i, s) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(s);
    }
    out.push(']');
}

fn int(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// The 1-based turn of each event, 0 before the first: an event whose
/// request id names a turn switches to it (the last turn with that id).
pub fn turn_numbers(p: &ParsedSession) -> Vec<usize> {
    let by_request: HashMap<&str, usize> = p
        .turns
        .iter()
        .enumerate()
        .map(|(i, t)| (t.request_id.as_str(), i + 1))
        .collect();
    p.events
        .iter()
        .scan(0, |cur, e| {
            if let Some(n) = e
                .request_id
                .as_deref()
                .filter(|r| !r.is_empty())
                .and_then(|r| by_request.get(r))
            {
                *cur = *n;
            }
            Some(*cur)
        })
        .collect()
}

/// The first event of the turn that crossed into the dumb zone, or -1.
pub fn cross_event(a: &AnalyzedSession) -> i64 {
    let Ok(turn) = usize::try_from(a.dumb_zone_cross_turn) else {
        return -1;
    };
    // A missing turn matches an event with no request id, as `findIndex` does.
    let want = a.parsed.turns.get(turn).map(|t| t.request_id.as_str());
    a.parsed
        .events
        .iter()
        .position(|e| e.request_id.as_deref() == want)
        .map_or(-1, int)
}

/// The first position holding a byte-identical body, per event.
pub fn body_ids(events: &[TimelineEvent]) -> Vec<usize> {
    let mut first: HashMap<&str, usize> = HashMap::with_capacity(events.len());
    events
        .iter()
        .enumerate()
        .map(|(pos, e)| *first.entry(e.body.as_str()).or_insert(pos))
        .collect()
}

/// Whether the analysis prints `f` on `t`.
fn turn_has(t: &Turn, f: TurnField) -> bool {
    t.shape == TurnShape::Claude || t.appended.contains(&f)
}

fn write_growth_turn(w: &mut Writer, t: &Turn) {
    let late = |w: &mut Writer, key: &str, f: TurnField, v: Option<f64>| {
        if let Some(v) = v.filter(|_| turn_has(t, f)) {
            w.key(key).num(v);
        }
    };
    w.begin_obj();
    w.key("model").text(&t.model);
    w.key("contextTokens").num(t.context_tokens);
    w.key("outputTokens").num(t.output_tokens);
    late(
        w,
        "thinkingTokens",
        TurnField::ThinkingTokens,
        t.thinking_tokens,
    );
    if let Some(e) = t.effort.as_ref().filter(|_| t.shape == TurnShape::Claude) {
        w.key("effort").text(e);
    }
    late(
        w,
        "webSearchRequests",
        TurnField::WebSearchRequests,
        t.web_search_requests,
    );
    late(
        w,
        "webFetchRequests",
        TurnField::WebFetchRequests,
        t.web_fetch_requests,
    );
    w.end_obj();
}

/// The growth body keys, no braces: compact turns and columnar snapshots.
fn growth_fields(a: &AnalyzedSession) -> String {
    let mut w = Writer::new(false);
    w.begin_obj();
    w.key("turns").begin_arr();
    a.parsed
        .turns
        .iter()
        .for_each(|t| write_growth_turn(w.elem(), t));
    w.end_arr();
    let column = |w: &mut Writer, key: &str, f: &dyn Fn(usize) -> f64| {
        w.key(key).begin_arr();
        (0..a.snapshots.len()).for_each(|i| w.elem().num(f(i)));
        w.end_arr();
    };
    let s = &a.snapshots;
    w.key("snapshots").begin_obj();
    w.key("turnIndex").begin_arr();
    for x in s {
        w.elem().int(int(x.turn_index));
    }
    w.end_arr();
    column(&mut w, "ctx", &|i| s[i].ctx);
    column(&mut w, "outputTokens", &|i| s[i].output_tokens);
    w.key("slices").begin_obj();
    for k in STACK {
        column(&mut w, k.as_str(), &|i| s[i].slices.get(k));
    }
    w.end_obj();
    w.end_obj();
    w.end_obj();
    let doc = w.finish();
    doc[1..doc.len() - 1].to_owned()
}

/// The summary keys, no braces: the analysis without events, turns and
/// snapshots.
fn summary_fields(a: &AnalyzedSession) -> String {
    let mut w = Writer::new(false);
    w.begin_obj();
    write_analysis_fields(&mut w, a, false);
    w.end_obj();
    let doc = w.finish();
    doc[1..doc.len() - 1].to_owned()
}

/// One history index row: the event without its text, plus its turn, body
/// length (UTF-16 units) and body id.
fn row_json(e: &TimelineEvent, turn: usize, body_id: usize) -> String {
    let mut w = Writer::new(false);
    w.begin_obj();
    write_event_fields(&mut w, e, true);
    w.key("turn").int(int(turn));
    w.key("bodyLen").int(int(units::len(&e.body)));
    w.key("bodyId").int(int(body_id));
    w.end_obj();
    w.finish()
}

fn event_json(e: &TimelineEvent) -> String {
    let mut w = Writer::new(false);
    w.begin_obj();
    write_event_fields(&mut w, e, false);
    w.end_obj();
    w.finish()
}

/// Every part of one analysis, ready to serve.
#[derive(Debug)]
pub struct SessionPages {
    analysis: AnalyzedSession,
    rev: String,
    events: Vec<String>,
    summary: String,
    growth: String,
    index: String,
    /// Lowercased `title\npreview\nbody` per event, built on the first search.
    search: OnceLock<Vec<String>>,
}

impl SessionPages {
    pub fn new(a: AnalyzedSession) -> Self {
        let events: Vec<String> = a.parsed.events.iter().map(event_json).collect();
        let page_hashes: Vec<String> = events.chunks(PAGE_SIZE).map(array_hash).collect();
        let turns = turn_numbers(&a.parsed);
        let ids = body_ids(&a.parsed.events);
        let rows: Vec<String> = a
            .parsed
            .events
            .iter()
            .enumerate()
            .map(|(pos, e)| row_json(e, turns[pos], ids[pos]))
            .collect();
        let index_hash = array_hash(&rows);
        let fields = growth_fields(&a);
        let growth_hash = hash_hex(&format!("{{{fields}}}"));
        let summary_fields = summary_fields(&a);
        // Every served part is in the hash, so it moves whenever one does.
        let rev = page_hashes
            .iter()
            .chain([&index_hash, &growth_hash])
            .fold(Fnv::new().write(summary_fields.as_bytes()), |h, x| {
                h.write(b",").write(x.as_bytes())
            })
            .hex();
        let mut index = format!(
            r#"{{"rev":"{rev}","hash":"{index_hash}","total":{},"crossEvent":{},"rows":"#,
            events.len(),
            cross_event(&a)
        );
        push_array(&mut index, &rows);
        index.push('}');
        let growth = format!(r#"{{"rev":"{rev}","hash":"{growth_hash}",{fields}}}"#);
        let pages = page_hashes
            .iter()
            .map(|h| format!(r#""{h}""#))
            .collect::<Vec<_>>()
            .join(",");
        let summary = format!(
            r#"{{{summary_fields},"history":{{"rev":"{rev}","total":{},"pageSize":{PAGE_SIZE},"pages":[{pages}],"index":"{index_hash}","growth":"{growth_hash}"}}}}"#,
            events.len()
        );
        Self {
            analysis: a,
            rev,
            events,
            summary,
            growth,
            index,
            search: OnceLock::new(),
        }
    }

    pub fn rev(&self) -> &str {
        &self.rev
    }

    pub fn total(&self) -> usize {
        self.events.len()
    }

    pub fn analysis(&self) -> &AnalyzedSession {
        &self.analysis
    }

    /// The analysis without events, turns and snapshots, plus `history`.
    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn growth(&self) -> &str {
        &self.growth
    }

    pub fn index(&self) -> &str {
        &self.index
    }

    /// Events `[offset, offset + limit)`, clamped to the total.
    pub fn page(&self, offset: usize, limit: usize) -> String {
        let total = self.events.len();
        let start = offset.min(total);
        let items = &self.events[start..offset.saturating_add(limit).min(total)];
        let mut out = format!(
            r#"{{"rev":"{}","offset":{offset},"total":{total},"hash":"{}","events":"#,
            self.rev,
            array_hash(items)
        );
        push_array(&mut out, items);
        out.push('}');
        out
    }

    /// Positions of events whose title, preview or body hold `q`, case
    /// folded as `toLowerCase` does.
    pub fn search(&self, q: &str) -> Vec<usize> {
        let hay = self.search.get_or_init(|| {
            self.analysis
                .parsed
                .events
                .iter()
                .map(|e| format!("{}\n{}\n{}", e.title, e.preview, e.body).to_lowercase())
                .collect()
        });
        let needle = text::escape_reserved(q).to_lowercase();
        hay.iter()
            .enumerate()
            .filter(|(_, h)| h.contains(&needle))
            .map(|(pos, _)| pos)
            .collect()
    }

    /// `{rev, positions}` for `positions`.
    pub fn search_json(&self, positions: &[usize]) -> String {
        let list: Vec<String> = positions.iter().map(usize::to_string).collect();
        format!(
            r#"{{"rev":"{}","positions":[{}]}}"#,
            self.rev,
            list.join(",")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_fnv1a_64() {
        assert_eq!(hash_hex(""), "cbf29ce484222325");
        assert_eq!(hash_hex("a"), "af63dc4c8601ec8c");
        let items = ["1".to_owned(), "{}".to_owned()];
        assert_eq!(array_hash(&items), hash_hex("[1,{}]"));
        assert_eq!(array_hash(&[]), hash_hex("[]"));
    }
}
