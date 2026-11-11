//! Fold Claude JSONL lines into a session header: timestamps, cwd, branch,
//! model, title, message count and estimated cost. No timeline is built.

use std::borrow::Cow;
use std::collections::HashMap;

use super::line::{Line, Usage, parse_line, parse_repaired};
use crate::js::{json, text, time};
use crate::model::{Agent, ProjectSlug, SessionHeader, SessionId, SourceTag};
use crate::pricing::{CostTokens, card_for, turn_cost};

/// Model id Claude Code writes on harness-made messages.
const SYNTHETIC_MODEL: &str = "<synthetic>";

/// The file facts a header carries besides its lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    pub id: SessionId,
    pub slug: ProjectSlug,
    pub path: String,
    pub size_bytes: u64,
    pub mtime_ms: i64,
    pub source: Option<SourceTag>,
}

/// One priced model call, folded over the lines that share its request id.
#[derive(Debug, Clone, PartialEq)]
struct Turn {
    /// Index into `HeaderAcc::models`.
    model: usize,
    tokens: CostTokens,
}

/// Running state while lines stream in.
#[derive(Debug, Default)]
pub struct HeaderAcc {
    started_at: Option<String>,
    updated_at: Option<String>,
    cwd: Option<String>,
    git_branch: Option<String>,
    model: Option<String>,
    title: Option<String>,
    users: u64,
    /// Each request id seen, with the index of its priced turn if it has one.
    requests: HashMap<String, Option<usize>>,
    line_keyed: u64,
    turns: Vec<Turn>,
    /// Distinct turn models, first seen first.
    models: Vec<String>,
    bad: u64,
    last_bad: bool,
}

/// Overwrite `slot` with `value`, reusing its allocation.
fn set(slot: &mut Option<String>, value: &str) {
    match slot {
        Some(s) => {
            s.clear();
            s.push_str(value);
        }
        None => *slot = Some(value.to_owned()),
    }
}

/// `s` as transcript text, unless it already is.
fn keep(s: &str, escaped: bool) -> Cow<'_, str> {
    if escaped {
        Cow::Borrowed(s)
    } else {
        text::escape_reserved(s)
    }
}

/// Token counts of a new call; a missing count reads 0.
fn call_tokens(u: &Usage) -> CostTokens {
    CostTokens {
        input: u.input_tokens.0.unwrap_or(0.0),
        output: u.output_tokens.0.unwrap_or(0.0),
        cache_read: u.cache_read_input_tokens.0.unwrap_or(0.0),
        cache_creation: u.cache_creation_input_tokens.0.unwrap_or(0.0),
        cache_creation_1h: u.cache_creation.ephemeral_1h_input_tokens.0,
    }
}

impl HeaderAcc {
    /// Fold one raw line (no trailing `\n`). Blank lines are skipped; a line
    /// that is not valid JSON counts as malformed.
    pub fn push_line(&mut self, line: &str) {
        if text::is_blank(line.as_bytes()) {
            return;
        }
        if let Ok(row) = parse_line(line) {
            return self.accept(&row, false);
        }
        if let Some(fixed) = json::repair(line, json::Overflow::Sentinel)
            && let Ok(row) = parse_repaired(&fixed)
        {
            return self.accept(&row, true);
        }
        self.bad += 1;
        self.last_bad = true;
    }

    /// `escaped` says the line's strings are already transcript text (see
    /// [`text::units`]); otherwise kept strings are escaped here.
    fn accept(&mut self, row: &Line<'_>, escaped: bool) {
        self.last_bad = false;
        if row.is_object {
            self.apply(row, escaped);
        }
    }

    fn apply(&mut self, o: &Line<'_>, escaped: bool) {
        let keep = |s| keep(s, escaped);
        if let Some(ts) = o.timestamp.non_empty() {
            let ts = keep(ts);
            if self.started_at.is_none() {
                self.started_at = Some(ts.clone().into_owned());
            }
            set(&mut self.updated_at, &ts);
        }
        if let Some(cwd) = o.cwd.get()
            && self.cwd.as_deref().is_none_or(str::is_empty)
        {
            set(&mut self.cwd, &keep(cwd));
        }
        if let Some(branch) = o.git_branch.get() {
            set(&mut self.git_branch, &keep(branch));
        }
        let assistant = o.kind.is("assistant");
        if assistant
            && self.model.as_deref().is_none_or(str::is_empty)
            && let Some(model) = o.message.model.get()
            && model != SYNTHETIC_MODEL
        {
            set(&mut self.model, &keep(model));
        }
        if o.kind.is("ai-title")
            && let Some(title) = o.ai_title.get()
        {
            set(&mut self.title, &keep(title));
        }
        if o.kind.is("user") {
            if !is_not_user_written(o) {
                self.users += 1;
            }
        } else if assistant {
            self.apply_assistant(o, escaped);
        }
    }

    /// Count the assistant message (one per request id, else one per line)
    /// and fold its usage into that request's turn.
    fn apply_assistant(&mut self, o: &Line<'_>, escaped: bool) {
        let request = o.request_id.get();
        let turn_at = if let Some(id) = request {
            if let Some(at) = self.requests.get(id) {
                *at
            } else {
                self.requests.insert(id.to_owned(), None);
                None
            }
        } else {
            self.line_keyed += 1;
            None
        };
        let usage = &o.message.usage;
        if !usage.is_object {
            return;
        }
        if let Some(at) = turn_at {
            let turn = &mut self.turns[at];
            turn.tokens.output = turn.tokens.output.max(usage.output_tokens.0.unwrap_or(0.0));
            return;
        }
        let model = o.message.model.get().unwrap_or("unknown");
        let tokens = call_tokens(usage);
        let context = tokens.input + tokens.cache_read + tokens.cache_creation;
        if context > 0.0 && model != SYNTHETIC_MODEL {
            if let Some(slot) = request.and_then(|id| self.requests.get_mut(id)) {
                *slot = Some(self.turns.len());
            }
            let model = self.model_index(&keep(model, escaped));
            self.turns.push(Turn { model, tokens });
        }
    }

    /// Index of `model` in `models`, adding it when new.
    fn model_index(&mut self, model: &str) -> usize {
        self.models
            .iter()
            .position(|m| m == model)
            .unwrap_or_else(|| {
                self.models.push(model.to_owned());
                self.models.len() - 1
            })
    }

    /// Bad lines, not counting a bad last line (a live session mid-write).
    fn malformed(&self) -> u64 {
        if self.last_bad {
            self.bad - 1
        } else {
            self.bad
        }
    }

    /// Sum priced turns in first-seen order; name the unpriced models. Each
    /// distinct model resolves to its card once.
    fn cost(&self) -> (Option<f64>, Option<Vec<String>>) {
        let cards: Vec<_> = self.models.iter().map(|m| card_for(m)).collect();
        let (usd, mut unpriced) = self.turns.iter().fold(
            (None::<f64>, Vec::<&str>::new()),
            |(usd, mut unpriced), t| {
                if let Some(c) = turn_cost(cards[t.model].as_ref(), &t.tokens) {
                    return (Some(usd.unwrap_or(0.0) + c), unpriced);
                }
                let model = self.models[t.model].as_str();
                if !unpriced.contains(&model) {
                    unpriced.push(model);
                }
                (usd, unpriced)
            },
        );
        unpriced.sort_by(|a, b| text::cmp_utf16(a, b));
        let unpriced =
            (!unpriced.is_empty()).then(|| unpriced.into_iter().map(str::to_owned).collect());
        (usd, unpriced)
    }

    /// The finished header.
    pub fn finish(self, meta: FileMeta) -> SessionHeader {
        let (cost_usd, unpriced_models) = self.cost();
        let malformed = self.malformed();
        let non_empty = |s: Option<String>| s.filter(|v| !v.is_empty());
        SessionHeader {
            id: meta.id,
            agent: Agent::Claude,
            path: meta.path,
            project: meta.slug,
            message_count: self.users + self.requests.len() as u64 + self.line_keyed,
            size_bytes: meta.size_bytes,
            updated_at: self
                .updated_at
                .unwrap_or_else(|| time::to_iso_string(meta.mtime_ms)),
            cwd: non_empty(self.cwd),
            git_branch: non_empty(self.git_branch),
            model: non_empty(self.model),
            title: non_empty(self.title),
            started_at: self.started_at,
            malformed_lines: (malformed > 0).then_some(malformed),
            cost_usd,
            unpriced_models,
            source: meta.source,
        }
    }
}

/// A user line the user did not write: harness-injected or tool output only.
fn is_not_user_written(o: &Line<'_>) -> bool {
    o.is_meta.0 || o.is_compact_summary.0 || o.message.content.0
}

/// Build a header from a whole transcript's text.
#[cfg(test)]
fn build_header(text: &str, meta: FileMeta) -> SessionHeader {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut acc = HeaderAcc::default();
    text.split('\n').for_each(|line| acc.push_line(line));
    acc.finish(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> FileMeta {
        FileMeta {
            id: SessionId::new("s"),
            slug: ProjectSlug::new("-p"),
            path: "/r/-p/s.jsonl".to_owned(),
            size_bytes: 10,
            mtime_ms: 0,
            source: None,
        }
    }

    #[test]
    fn empty_text_uses_mtime() {
        let h = build_header("  \n", meta());
        assert_eq!(h.message_count, 0);
        assert_eq!(h.updated_at, "1970-01-01T00:00:00.000Z");
        assert_eq!(h.started_at, None);
    }

    #[test]
    fn folds_fields() {
        let text = [
            r#"{"type":"user","timestamp":"2026-01-01T10:00:00.000Z","cwd":"","gitBranch":"main","message":{"content":"hi"}}"#,
            r#"{"type":"user","isMeta":true,"cwd":"/w"}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result"}]}}"#,
            r#"{"type":"assistant","requestId":"r1","message":{"model":"<synthetic>","usage":{"input_tokens":5}}}"#,
            r#"{"type":"assistant","requestId":"r2","message":{"model":"claude-opus-5-5","usage":{"input_tokens":1000,"output_tokens":10,"cache_read_input_tokens":2000,"cache_creation_input_tokens":3000,"cache_creation":{"ephemeral_1h_input_tokens":1000}}}}"#,
            r#"{"type":"assistant","requestId":"r2","message":{"model":"claude-opus-5-5","usage":{"input_tokens":1,"output_tokens":50}}}"#,
            r#"{"type":"assistant","message":{"model":"mystery-model","usage":{"input_tokens":1}}}"#,
            r#"{"type":"ai-title","aiTitle":"First"}"#,
            r#"{"type":"ai-title","aiTitle":"Second title"}"#,
            "not json",
            r#"{"timestamp":"2026-01-01T09:00:00.000Z","gitBranch":"feat"}"#,
            "{\"partial\":",
        ]
        .join("\n");
        let h = build_header(&text, meta());
        assert_eq!(h.message_count, 4);
        assert_eq!(h.cwd.as_deref(), Some("/w"));
        assert_eq!(h.git_branch.as_deref(), Some("feat"));
        assert_eq!(h.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(h.title.as_deref(), Some("Second title"));
        assert_eq!(h.started_at.as_deref(), Some("2026-01-01T10:00:00.000Z"));
        assert_eq!(h.updated_at, "2026-01-01T09:00:00.000Z");
        assert_eq!(h.malformed_lines, Some(1));
        assert_eq!(h.cost_usd, Some(0.0234));
        assert_eq!(h.unpriced_models, Some(vec!["mystery-model".to_owned()]));
    }

    #[test]
    fn bom_and_crlf() {
        let h = build_header(
            "\u{FEFF}{\"type\":\"user\"}\r\n{\"type\":\"user\"}\r\n",
            meta(),
        );
        assert_eq!(h.message_count, 2);
        assert_eq!(h.malformed_lines, None);
    }

    #[test]
    fn request_seen_before_its_usage() {
        let text = [
            r#"{"type":"assistant","requestId":"r1","message":{"model":"claude-opus-5-5"}}"#,
            r#"{"type":"assistant","requestId":"r1","message":{"model":"claude-opus-5-5","usage":{"input_tokens":1000,"output_tokens":10}}}"#,
            r#"{"type":"assistant","requestId":"r1","message":{"model":"claude-opus-5-5","usage":{"input_tokens":1000,"output_tokens":50}}}"#,
            r#"{"type":"assistant","requestId":"r2","message":{"model":"mystery","usage":{"input_tokens":1}}}"#,
            r#"{"type":"assistant","requestId":"r3","message":{"model":"mystery","usage":{"input_tokens":1}}}"#,
        ]
        .join("\n");
        let h = build_header(&text, meta());
        assert_eq!(h.message_count, 3);
        let one_turn = turn_cost(
            card_for("claude-opus-5-5").as_ref(),
            &CostTokens {
                input: 1000.0,
                output: 50.0,
                ..CostTokens::default()
            },
        );
        assert_eq!(h.cost_usd, one_turn);
        assert_eq!(h.unpriced_models, Some(vec!["mystery".to_owned()]));
    }

    #[test]
    fn plane_16_chars_stay_real() {
        let decoded = |s: Option<String>| text::units(&s.unwrap_or_default()).collect::<Vec<_>>();
        let real = build_header(
            "{\"type\":\"ai-title\",\"aiTitle\":\"x\u{10F801}\"}",
            meta(),
        );
        assert_eq!(
            decoded(real.title),
            [text::Unit::Char('x'), text::Unit::Char('\u{10F801}')]
        );
        let mixed = build_header(
            "{\"type\":\"ai-title\",\"aiTitle\":\"\\ud801\u{10F801}\"}",
            meta(),
        );
        assert_eq!(
            decoded(mixed.title),
            [text::Unit::Lone(0xD801), text::Unit::Char('\u{10F801}')]
        );
    }

    #[test]
    fn out_of_range_usage_keeps_the_line() {
        let text = [
            r#"{"type":"assistant","requestId":"a","message":{"model":"claude-opus-5-5","usage":{"input_tokens":1e400}}}"#,
            r#"{"type":"user"}"#,
        ]
        .join("\n");
        let h = build_header(&text, meta());
        assert_eq!(h.message_count, 2);
        assert_eq!(h.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(h.malformed_lines, None);
        assert!(h.cost_usd.is_some_and(f64::is_infinite));
    }

    #[test]
    fn line_keys_follow_parsed_row_index() {
        let text = "42\n{\"type\":\"assistant\"}\n{\"type\":\"assistant\"}";
        assert_eq!(build_header(text, meta()).message_count, 2);
    }
}
