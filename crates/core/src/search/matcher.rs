//! Line matcher for transcript search. A line is a candidate when its raw
//! text holds the query (a cheap prefilter). Only candidates are parsed; the
//! match must then sit in a string value that is real content, not a key, an
//! id or a metadata field.

use std::collections::HashSet;

use super::fold::fold_literal;
use super::snippet::{Snippet, build_snippet};
use crate::export::home::HomeCollapser;
use crate::js::json_out;
use crate::js::regex::Re;
use crate::js::text;
use crate::js::value::JsValue;

/// Keys whose values are metadata or encoded blobs, at any depth.
const IGNORED_KEYS: &[&str] = &[
    "approval_policy",
    "branch",
    "call_id",
    "cli_version",
    "cwd",
    "data",
    "directory",
    "effort",
    "encrypted_content",
    "entrypoint",
    "git",
    "gitBranch",
    "id",
    "kind",
    "leafUuid",
    "messageId",
    "model",
    "model_provider",
    "modelId",
    "originator",
    "parentId",
    "parentUuid",
    "permissionMode",
    "phase",
    "projectId",
    "provider",
    "repository_url",
    "requestId",
    "role",
    "service_tier",
    "sessionId",
    "signature",
    "slug",
    "stop_reason",
    "stop_sequence",
    "stopReason",
    "timestamp",
    "tool_call_id",
    "tool_use_id",
    "toolCallId",
    "type",
    "usage",
    "userType",
    "uuid",
    "version",
];

/// Record types that hold only run settings, never conversation text.
const METADATA_RECORDS: [&str; 2] = ["session_meta", "turn_context"];

const MAX_DEPTH: usize = 64;

/// Separator between words in raw JSON text: whitespace or its JSON escape,
/// case-folded as a JS `i` pattern reads `\\[nrtf]` and `\\u000[bB]`.
const RAW_SEP: &str = r"(?:\s|\\[nrtfNRTF]|\\[uU]000[bB])+";

/// One search hit: a line number and the snippet around the match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub line: usize,
    pub snippet: String,
    pub match_start: Option<usize>,
    pub match_end: Option<usize>,
}

/// Matching lines of one transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptMatch {
    pub hits: Vec<SearchHit>,
    /// Every matching line, including those not kept as hits.
    pub match_count: usize,
}

/// The two case-insensitive patterns of a query. `content` runs on decoded
/// text; `raw` runs on a JSON line and also accepts the JSON-escaped form of
/// each word, so it never rejects a line `content` would accept.
#[derive(Debug)]
pub struct Patterns {
    pub content: Re,
    pub raw: Re,
}

/// `q.split(/\s+/)` without empty words.
pub fn words(q: &str) -> Vec<&str> {
    q.split(text::is_js_whitespace)
        .filter(|w| !w.is_empty())
        .collect()
}

/// `JSON.stringify(w).slice(1, -1)`.
fn json_escaped(w: &str) -> String {
    let mut out = String::new();
    json_out::push_text(&mut out, w);
    out[1..out.len() - 1].to_owned()
}

/// A pattern that never matches, for a query too big to compile.
fn never() -> Re {
    Re::new("x^", "")
}

fn compile(src: &str) -> Re {
    Re::try_new(src, "").unwrap_or_else(|_| never())
}

/// Case-insensitive patterns for `q`. Whitespace runs in `q` match any
/// whitespace run.
pub fn query_patterns(q: &str) -> Patterns {
    let parts = words(q);
    let content = parts
        .iter()
        .map(|w| fold_literal(w))
        .collect::<Vec<_>>()
        .join(r"\s+");
    let raw = parts
        .iter()
        .map(|w| {
            let plain = fold_literal(w);
            let escaped = json_escaped(w);
            if escaped == *w {
                plain
            } else {
                format!("(?:{plain}|{})", fold_literal(&escaped))
            }
        })
        .collect::<Vec<_>>()
        .join(RAW_SEP);
    Patterns {
        content: compile(&content),
        raw: compile(&raw),
    }
}

/// String values worth searching, in document order.
fn collect_text<'a>(value: &'a JsValue, out: &mut Vec<&'a str>, depth: usize) {
    match value {
        JsValue::Str(s) => out.push(s),
        JsValue::Arr(items) if depth < MAX_DEPTH => {
            items.iter().for_each(|v| collect_text(v, out, depth + 1))
        }
        JsValue::Obj(o) if depth < MAX_DEPTH => o
            .entries()
            .filter(|(k, _)| !IGNORED_KEYS.contains(k))
            .for_each(|(_, v)| collect_text(v, out, depth + 1)),
        _ => {}
    }
}

/// The searchable text of one JSONL line, or `None` when it is not JSON or
/// is a metadata record.
fn content_of(line: &str) -> Option<String> {
    let record = JsValue::parse(line)?;
    let kind = record.get("type").and_then(JsValue::as_str);
    if kind.is_some_and(|k| METADATA_RECORDS.contains(&k)) {
        return None;
    }
    let mut parts = Vec::new();
    collect_text(&record, &mut parts, 0);
    Some(parts.join("\n"))
}

/// A compiled query, reused across transcripts.
#[derive(Debug)]
pub struct Matcher {
    patterns: Option<Patterns>,
}

impl Matcher {
    /// A blank `q` matches nothing.
    pub fn new(q: &str) -> Self {
        Self {
            patterns: (!words(q).is_empty()).then(|| query_patterns(q)),
        }
    }

    /// Matching lines of `text`. Hits are distinct by snippet (agents repeat
    /// the same text in several records) and capped at `max_hits`.
    pub fn run(
        &self,
        text: &str,
        redact: bool,
        max_hits: usize,
        collapser: &HomeCollapser,
    ) -> TranscriptMatch {
        let mut out = TranscriptMatch {
            hits: Vec::new(),
            match_count: 0,
        };
        let Some(Patterns { content, raw }) = &self.patterns else {
            return out;
        };
        if !raw.is_match(text) {
            return out;
        }
        let mut seen: HashSet<String> = HashSet::new();
        for (i, row) in text.split('\n').enumerate() {
            if !raw.is_match(row) {
                continue;
            }
            let Some(found) = content_of(row).filter(|c| content.is_match(c)) else {
                continue;
            };
            out.match_count += 1;
            if out.hits.len() >= max_hits {
                continue;
            }
            let Snippet { text, range } = build_snippet(&found, content, redact, collapser);
            if seen.insert(text.clone()) {
                out.hits.push(SearchHit {
                    line: i + 1,
                    snippet: text,
                    match_start: range.map(|r| r.0),
                    match_end: range.map(|r| r.1),
                });
            }
        }
        out
    }
}

/// Arguments of [`match_transcript`].
pub struct MatchArgs<'a> {
    pub text: &'a str,
    pub q: &'a str,
    pub redact: bool,
    pub max_hits: usize,
    pub collapser: &'a HomeCollapser,
}

/// Matching lines of one transcript for a one-off query.
pub fn match_transcript(args: &MatchArgs<'_>) -> TranscriptMatch {
    Matcher::new(args.q).run(args.text, args.redact, args.max_hits, args.collapser)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_pattern_spells_both_forms() {
        let p = query_patterns("a\"b  c\\d");
        assert!(p.raw.is_match(r#"a\"b\nc\\d"#));
        assert!(p.raw.is_match(r#"A"B \N C\D"#));
        assert!(p.content.is_match("a\"b \n c\\d"));
        assert!(!p.content.is_match("a\"bc\\d"));
    }

    #[test]
    fn ignores_metadata_and_keys() {
        assert_eq!(content_of(r#"{"type":"turn_context","x":"y"}"#), None);
        assert_eq!(
            content_of(r#"{"b":"walrus b","1":"walrus one","id":"no"}"#).as_deref(),
            Some("walrus one\nwalrus b")
        );
        assert_eq!(content_of(r#""top""#).as_deref(), Some("top"));
    }

    #[test]
    fn depth_limit() {
        let at = |n: usize| format!("{}\"walrus\"{}", "[".repeat(n), "]".repeat(n));
        assert_eq!(content_of(&at(64)).as_deref(), Some("walrus"));
        assert_eq!(content_of(&at(65)).as_deref(), Some(""));
    }
}
