//! The rule check: which memory-file lines already cover a cause row, and
//! whether the cause kept firing after the rule appeared.

use std::collections::HashSet;

use super::causes::{GIT_DIFF_FIX, escape_re, git_diff_probes, probes, spec};
use crate::js::json_out::Writer;
use crate::js::num::js_round;
use crate::js::regex::Re;
use crate::js::time::to_iso_string;
use crate::model::Agent;
use crate::stats::rank::{desc, safe_div};
use crate::stats::redact::Redactor;
use crate::stats::rule_dates::LineDates;
use crate::stats::schema::CauseId;

/// A count split by who made the call.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Split {
    pub main: f64,
    pub nested: f64,
}

impl Split {
    pub fn total(self) -> f64 {
        self.main + self.nested
    }

    pub fn write(self, w: &mut Writer) {
        w.begin_obj();
        w.key("main").num(self.main);
        w.key("nested").num(self.nested);
        w.end_obj();
    }
}

/// Codex's walk rules for a project-dir cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexCover {
    pub exact: bool,
    pub stops: Vec<String>,
}

/// Which sessions a memory file reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleCover {
    pub agent: Agent,
    pub codex: Option<CodexCover>,
    pub dir: Option<String>,
    /// `(id, is_default)` when the agent has 2+ sources.
    pub source: Option<(String, bool)>,
}

/// One memory file as the rule check reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryDoc {
    pub cover: RuleCover,
    pub display_path: String,
    pub lines: Vec<String>,
    pub mtime_ms: Option<f64>,
    pub real_path: String,
}

/// One file's reach and the UTC day after which its hits count (`""` = all).
pub struct CoverSince<'a> {
    pub after_day: String,
    pub cover: &'a RuleCover,
}

/// What the rule check reads back from the cause table.
pub trait CauseCounts {
    fn since(&self, key: &str, covers: &[CoverSince<'_>]) -> (Split, Split);
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleMatch {
    pub path: String,
    pub line: f64,
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleCheck {
    pub matches: Vec<RuleMatch>,
    pub since_iso: Option<String>,
    pub hits_since: Split,
    pub calls_since: Split,
    pub per1k_since: Split,
    pub advice: String,
}

impl RuleCheck {
    pub fn write(&self, w: &mut Writer) {
        w.begin_obj();
        w.key("matches").begin_arr();
        for m in &self.matches {
            w.elem().begin_obj();
            w.key("path").text(&m.path);
            w.key("line").num(m.line);
            w.key("excerpt").text(&m.excerpt);
            w.end_obj();
        }
        w.end_arr();
        match &self.since_iso {
            Some(s) => w.key("sinceIso").str(s),
            None => w.key("sinceIso").null(),
        }
        w.key("hitsSince");
        self.hits_since.write(w);
        w.key("callsSince");
        self.calls_since.write(w);
        w.key("per1kSince");
        self.per1k_since.write(w);
        w.key("advice").text(&self.advice);
        w.end_obj();
    }
}

/// Hits per 1000 calls, by caller.
pub fn per1k_of(hits: Split, calls: Split) -> Split {
    let r = |n: f64| crate::stats::rank::round(n);
    Split {
        main: r(safe_div(hits.main * 1000.0, calls.main)),
        nested: r(safe_div(hits.nested * 1000.0, calls.nested)),
    }
}

/// What the rule check needs of a cause row.
pub struct RowRef<'a> {
    pub key: &'a str,
    pub cause: CauseId,
    pub fix: Option<&'a str>,
    pub details: &'a [(String, f64)],
}

enum Probes {
    Owned(Vec<Vec<Re>>),
    Static(&'static [Vec<Re>]),
    Joined(&'static [Vec<Re>], &'static [Vec<Re>]),
}

impl Probes {
    fn iter(&self) -> Box<dyn Iterator<Item = &Vec<Re>> + '_> {
        match self {
            Self::Owned(v) => Box::new(v.iter()),
            Self::Static(v) => Box::new(v.iter()),
            Self::Joined(a, b) => Box::new(a.iter().chain(b.iter())),
        }
    }

    fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }
}

fn binary_probes(details: &[(String, f64)]) -> Vec<Vec<Re>> {
    details
        .iter()
        .map(|(value, _)| {
            let name = escape_re(value);
            let start = r"(?<![\w.-])";
            let end = r"(?=\s|$|[,;:)]|\.(?:\s|$))";
            let b = format!("{start}`?{name}`?{end}");
            let absent = [
                format!(r"\b(?:has|have|there is|there's|there are) no {b}"),
                format!(r"\bno `{name}`{end}"),
                format!(r"\bno {b} (?:binary|command|executable|installed|on (?:this )?(?:host|machine|box|PATH))\b"),
            ]
            .join("|");
            vec![Re::new(
                &format!(r"{absent}|{b} (?:is )?(?:not installed|missing|unavailable|not available|not on PATH)|\b(?:install|without|instead of) {b}"),
                "i",
            )]
        })
        .collect()
}

fn probes_of(row: &RowRef<'_>) -> Probes {
    if row.cause == CauseId::CommandNotFound {
        return Probes::Owned(binary_probes(row.details));
    }
    let base = probes(row.cause);
    if row.cause == CauseId::OutputTooLarge && row.fix == Some(GIT_DIFF_FIX) {
        Probes::Joined(base, git_diff_probes())
    } else {
        Probes::Static(base)
    }
}

const MATCH_CAP: usize = 3;
const EXCERPT_MAX: usize = 160;
const NESTED_SHARE: f64 = 0.8;
const ADVICE_MAX: usize = 400;

fn match_doc(doc: &MemoryDoc, probes: &Probes) -> Vec<usize> {
    let accepts = |text: &str| probes.iter().any(|p| p.iter().all(|re| re.is_match(text)));
    let single: Vec<usize> = doc
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| accepts(l))
        .map(|(i, _)| i)
        .collect();
    if !single.is_empty() {
        return single;
    }
    let mut hits: Vec<usize> = Vec::new();
    for (i, line) in doc.lines.iter().enumerate() {
        let window = doc.lines[i.saturating_sub(1)..(i + 2).min(doc.lines.len())].join("\n");
        let anchored = probes.iter().any(|p| {
            p.first().is_some_and(|re| re.is_match(line)) && p.iter().all(|re| re.is_match(&window))
        });
        if anchored && !(i > 0 && hits.last() == Some(&(i - 1))) {
            hits.push(i);
        }
    }
    hits
}

#[expect(clippy::cast_possible_truncation)]
fn day_iso(ms: f64) -> String {
    to_iso_string(ms as i64)[..10].to_owned()
}

fn advice_of(cause: CauseId, hits: Split, calls: Split) -> String {
    let base = spec(cause)
        .advice
        .unwrap_or("move the rule into the prompt that keeps hitting it");
    if calls.total() == 0.0 {
        return format!("if it keeps firing: {base}");
    }
    let n = hits.total();
    if n == 0.0 {
        return "no hits on whole days since the rule appeared".to_owned();
    }
    let nested = safe_div(hits.nested, n);
    if nested >= NESTED_SHARE {
        format!(
            "{base}. {}% of hits come from subagents or workflow agents, which may not load your memory file: put the rule in the agent or workflow prompt",
            crate::js::num::to_js_string(js_round(nested * 100.0))
        )
    } else {
        base.to_owned()
    }
}

fn since_of(doc: &MemoryDoc, lines: &[usize], dates: &LineDates) -> Option<f64> {
    lines
        .iter()
        .try_fold(None::<f64>, |min, &i| {
            let at = dates.date_of(doc, i)?;
            Some(Some(min.map_or(at, |m| m.min(at))))
        })
        .flatten()
}

/// The rule check for one ranked cause row, or `None` when no file covers it.
#[expect(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn check_row(
    row: &RowRef<'_>,
    docs: &[MemoryDoc],
    counts: &dyn CauseCounts,
    dates: &LineDates,
    redact: &Redactor,
) -> Option<RuleCheck> {
    let probes = probes_of(row);
    if probes.is_empty() {
        return None;
    }
    let reaches = |doc: &MemoryDoc| {
        counts
            .since(
                row.key,
                &[CoverSince {
                    after_day: String::new(),
                    cover: &doc.cover,
                }],
            )
            .0
            .total()
            > 0.0
    };
    let mut found: Vec<(&MemoryDoc, Vec<usize>, Option<f64>)> = docs
        .iter()
        .filter_map(|doc| {
            let lines = match_doc(doc, &probes);
            (!lines.is_empty() && reaches(doc)).then(|| {
                let since = since_of(doc, &lines, dates);
                (doc, lines, since)
            })
        })
        .collect();
    found.sort_by(|a, b| desc(a.2.unwrap_or(-1.0), b.2.unwrap_or(-1.0)));
    let newest = found.first()?.2;
    let after_day = newest.map_or_else(String::new, day_iso);
    let covers: Vec<CoverSince<'_>> = found
        .iter()
        .map(|(doc, _, _)| CoverSince {
            after_day: after_day.clone(),
            cover: &doc.cover,
        })
        .collect();
    let (hits, calls) = counts.since(row.key, &covers);
    let mut listed = HashSet::new();
    let matches: Vec<RuleMatch> = found
        .iter()
        .flat_map(|(doc, lines, _)| lines.iter().map(move |&i| (*doc, i)))
        .filter(|(doc, i)| listed.insert((doc.real_path.clone(), *i)))
        .take(MATCH_CAP)
        .map(|(doc, i)| RuleMatch {
            path: doc.display_path.clone(),
            line: (i + 1) as f64,
            excerpt: redact.line_max(&doc.lines[i], EXCERPT_MAX),
        })
        .collect();
    Some(RuleCheck {
        matches,
        since_iso: newest.map(|n| to_iso_string(n as i64)),
        hits_since: hits,
        calls_since: calls,
        per1k_since: per1k_of(hits, calls),
        advice: redact.line_max(&advice_of(row.cause, hits, calls), ADVICE_MAX),
    })
}
