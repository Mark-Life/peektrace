//! Fold Claude JSONL lines into a session header: timestamps, cwd, branch,
//! model, title, message count and estimated cost. No timeline is built.

use std::collections::{HashMap, HashSet};

use super::line::{Line, Usage, parse_line, replace_lone_surrogates};
use crate::js::{text, time};
use crate::model::{Agent, ProjectSlug, SessionHeader, SessionId, SourceTag};
use crate::pricing::{CostTokens, turn_cost};

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
    model: String,
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
    request_ids: HashSet<String>,
    line_keyed: u64,
    turns: Vec<Turn>,
    turn_by_request: HashMap<String, usize>,
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
            return self.accept(&row);
        }
        if let Some(fixed) = replace_lone_surrogates(line)
            && let Ok(row) = parse_line(&fixed)
        {
            return self.accept(&row);
        }
        self.bad += 1;
        self.last_bad = true;
    }

    fn accept(&mut self, row: &Line<'_>) {
        self.last_bad = false;
        if row.is_object {
            self.apply(row);
        }
    }

    fn apply(&mut self, o: &Line<'_>) {
        if let Some(ts) = o.timestamp.non_empty() {
            if self.started_at.is_none() {
                self.started_at = Some(ts.to_owned());
            }
            set(&mut self.updated_at, ts);
        }
        if let Some(cwd) = o.cwd.get()
            && self.cwd.as_deref().is_none_or(str::is_empty)
        {
            set(&mut self.cwd, cwd);
        }
        if let Some(branch) = o.git_branch.get() {
            set(&mut self.git_branch, branch);
        }
        let assistant = o.kind.is("assistant");
        if assistant
            && self.model.as_deref().is_none_or(str::is_empty)
            && let Some(model) = o.message.model.get()
            && model != SYNTHETIC_MODEL
        {
            set(&mut self.model, model);
        }
        if o.kind.is("ai-title")
            && let Some(title) = o.ai_title.get()
        {
            set(&mut self.title, title);
        }
        if o.kind.is("user") {
            if !is_not_user_written(o) {
                self.users += 1;
            }
        } else if assistant {
            self.apply_assistant(o);
        }
    }

    /// Count the assistant message (one per request id, else one per line)
    /// and fold its usage into that request's turn.
    fn apply_assistant(&mut self, o: &Line<'_>) {
        let request = o.request_id.get();
        match request {
            Some(id) if !self.request_ids.contains(id) => {
                self.request_ids.insert(id.to_owned());
            }
            Some(_) => {}
            None => self.line_keyed += 1,
        }
        let usage = &o.message.usage;
        if !usage.is_object {
            return;
        }
        let model = o.message.model.get().unwrap_or("unknown");
        if let Some(&at) = request.and_then(|id| self.turn_by_request.get(id)) {
            let turn = &mut self.turns[at];
            turn.tokens.output = turn.tokens.output.max(usage.output_tokens.0.unwrap_or(0.0));
            return;
        }
        let tokens = call_tokens(usage);
        let context = tokens.input + tokens.cache_read + tokens.cache_creation;
        if context > 0.0 && model != SYNTHETIC_MODEL {
            if let Some(id) = request {
                self.turn_by_request.insert(id.to_owned(), self.turns.len());
            }
            self.turns.push(Turn {
                model: model.to_owned(),
                tokens,
            });
        }
    }

    /// Bad lines, not counting a bad last line (a live session mid-write).
    fn malformed(&self) -> u64 {
        if self.last_bad {
            self.bad - 1
        } else {
            self.bad
        }
    }

    /// Sum priced turns in first-seen order; name the unpriced models.
    fn cost(&self) -> (Option<f64>, Option<Vec<String>>) {
        let (usd, mut unpriced) = self.turns.iter().fold(
            (None::<f64>, Vec::<&str>::new()),
            |(usd, mut unpriced), t| {
                if let Some(c) = turn_cost(&t.model, &t.tokens) {
                    return (Some(usd.unwrap_or(0.0) + c), unpriced);
                }
                if !unpriced.contains(&t.model.as_str()) {
                    unpriced.push(&t.model);
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
            message_count: self.users + self.request_ids.len() as u64 + self.line_keyed,
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
pub fn build_header(text: &str, meta: FileMeta) -> SessionHeader {
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
    fn line_keys_follow_parsed_row_index() {
        let text = "42\n{\"type\":\"assistant\"}\n{\"type\":\"assistant\"}";
        assert_eq!(build_header(text, meta()).message_count, 2);
    }
}
