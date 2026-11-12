//! What a retry changed: the word diff between a failed command and its
//! retry, read around the cause's culprit.

use std::collections::HashSet;

use super::similarity::ratio;
use super::waiting::wait_shape;
use crate::js::regex::LazyRe;
use crate::js::{text, units};
use crate::js_re;
use crate::stats::schema::{CallFact, CauseId, WaitShape};
use crate::stats::shell_ctx::{Ctx, shell_contexts};
use crate::stats::shell_lex::{bin_name, command_index};

const DIFF_MAX: usize = 80;
const CLIP_MARK: &str = "…";
const REWRITE_MIN: f64 = 0.5;

/// The token change a retry made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedDiff {
    pub from: String,
    pub to: String,
}

/// Words split at code or heredoc whitespace and at `;&|`.
fn quoted_words(cmd: &str) -> Vec<String> {
    let ctx = shell_contexts(cmd);
    let mut words = Vec::new();
    let mut buf = String::new();
    for (i, c) in cmd.char_indices() {
        let splits = matches!(ctx[i], Ctx::Code | Ctx::Heredoc);
        if splits && (text::is_js_whitespace(c) || matches!(c, ';' | '&' | '|')) {
            if !buf.is_empty() {
                words.push(std::mem::take(&mut buf));
            }
        } else {
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        words.push(buf);
    }
    words
}

fn unclipped_words(cmd: &str) -> Vec<String> {
    let mut w = quoted_words(cmd);
    if cmd.ends_with(CLIP_MARK) {
        w.pop();
    }
    w
}

#[expect(clippy::many_single_char_names, reason = "LCS table indexes")]
fn word_diff(a: &[String], b: &[String]) -> (Vec<String>, Vec<String>) {
    let mut t = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            t[i][j] = if a[i] == b[j] {
                t[i + 1][j + 1] + 1
            } else {
                t[i + 1][j].max(t[i][j + 1])
            };
        }
    }
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
        } else if t[i + 1][j] >= t[i][j + 1] {
            removed.push(a[i].clone());
            i += 1;
        } else {
            added.push(b[j].clone());
            j += 1;
        }
    }
    removed.extend_from_slice(&a[i..]);
    added.extend_from_slice(&b[j..]);
    (removed, added)
}

fn clip_diff(s: &str) -> String {
    units::clip(s, DIFF_MAX)
}

pub(super) fn diff_of_sides(from: &str, to: &str) -> Option<LearnedDiff> {
    let (a, b) = (clip_diff(from), clip_diff(to));
    (a != b).then_some(LearnedDiff { from: a, to: b })
}

fn rewrite_of<'a>(from: &str, added: &'a [String]) -> Option<(&'a str, f64)> {
    let mut found: Option<(&str, f64)> = None;
    for w in added {
        let score = ratio(from, w);
        if score >= REWRITE_MIN && found.is_none_or(|f| score > f.1) {
            found = Some((w, score));
        }
    }
    found
}

pub(super) type Culprit<'a> = Box<dyn Fn(&str) -> bool + 'a>;

static EQUALS_WORD: LazyRe = js_re!(r#"^["']?="#);

pub(super) fn culprit_of(cause: CauseId, arg: Option<&str>) -> Option<Culprit<'_>> {
    match cause {
        CauseId::ZshEqualsAbort => Some(Box::new(|w: &str| EQUALS_WORD.is_match(w))),
        CauseId::PkillSelfKill => Some(Box::new(|w: &str| matches!(w, "pkill" | "pgrep" | "kill"))),
        _ => arg.map(|a| Box::new(move |w: &str| w.contains(a)) as Culprit<'_>),
    }
}

pub(super) fn learned_diff(
    failed_cmd: &str,
    retry_cmd: &str,
    culprit: Option<Culprit<'_>>,
) -> Option<LearnedDiff> {
    let (rem, add) = word_diff(&unclipped_words(failed_cmd), &unclipped_words(retry_cmd));
    let moved: HashSet<&String> = rem.iter().filter(|w| add.contains(w)).collect();
    let removed: Vec<String> = rem.iter().filter(|w| !moved.contains(w)).cloned().collect();
    let added: Vec<String> = add.iter().filter(|w| !moved.contains(w)).cloned().collect();
    if let Some(pick) = culprit {
        let c = removed.iter().find(|w| pick(w))?;
        return diff_of_sides(c, rewrite_of(c, &added).map_or("(dropped)", |r| r.0));
    }
    let mut best: Option<(&str, &str, f64)> = None;
    for from in &removed {
        if let Some((to, score)) = rewrite_of(from, &added)
            && best.is_none_or(|b| score > b.2)
        {
            best = Some((from, to, score));
        }
    }
    best.and_then(|(f, t, _)| diff_of_sides(f, t))
}

static OPENER: LazyRe = js_re!(r"^(?:\$\(|[({`])+");
const LEAD_WORDS: [&str; 7] = ["!", "do", "else", "if", "then", "until", "while"];
const BUILTINS: [&str; 16] = [
    ".", "alias", "cd", "eval", "exit", "export", "false", "hash", "popd", "pushd", "rehash",
    "return", "set", "source", "true", "unset",
];

pub(super) fn command_word_of(s: &str) -> Option<String> {
    let words: Vec<String> = s
        .split(' ')
        .map(|raw| OPENER.replace_first(raw, |_| String::new()))
        .filter(|w| !w.is_empty())
        .collect();
    let lead = words.iter().position(|w| !LEAD_WORDS.contains(&w.as_str()));
    let rest = lead.map_or(&[][..], |l| &words[l..]);
    let word = rest.get(command_index(rest))?;
    let name = bin_name(word);
    (!BUILTINS.contains(&name)).then(|| name.to_owned())
}

static SLEEP_WORD: LazyRe = js_re!(r"\bsleep\s+\.?\d+(?:\.\d+)?");
static SHELL_PAYLOAD: LazyRe = js_re!(r#"\b(?:ba|z)?sh\s+-\w*c\s+(['"])([\s\S]*?)\1"#, "g");

fn wait_in(cmd: &str, wait: Option<WaitShape>) -> Option<WaitShape> {
    wait.or_else(|| {
        SHELL_PAYLOAD
            .captures_all(cmd)
            .iter()
            .find_map(|m| wait_shape(m.get(2).unwrap_or("")))
    })
}

pub(super) fn wait_of(retry: &CallFact) -> String {
    let Some(b) = &retry.bash else {
        return "(no wait)".into();
    };
    if b.bg {
        return "run_in_background".into();
    }
    match wait_in(&b.cmd, b.wait) {
        Some(WaitShape::LoopPoll) => "poll loop".into(),
        Some(WaitShape::Watcher) => "watcher".into(),
        Some(WaitShape::CurlRetry) => "curl --retry".into(),
        Some(WaitShape::BareDelay) => SLEEP_WORD
            .find(&b.cmd)
            .map_or_else(|| "sleep".to_owned(), |r| b.cmd[r].to_owned()),
        None => "(no wait)".into(),
    }
}
