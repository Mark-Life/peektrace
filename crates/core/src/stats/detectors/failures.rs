//! Failures: the exit-0 output scanner, the benign filter, and the fail table.

use std::cmp::Ordering;

use indexmap::IndexMap;

use super::causes::{is_shell_tool, spec};
use crate::js::json_out::Writer;
use crate::js::regex::LazyRe;
use crate::js::{text, units};
use crate::js_re;
use crate::stats::accumulate::{
    DrillHit, DrillTable, DrillTarget, FinishCtx, SampleRef, Tally, hit_of, redact_hits,
    sample_line, sample_of, samples_of, write_samples,
};
use crate::stats::rank::{asc, bump, cmp_str, desc, rank, round_to, share, top_key};
use crate::stats::schema::{CallFact, Fact, WriteExt};

const PIPED_EXIT_CODE: &str = "piped-exit-code";

pub static LINT_RE: LazyRe =
    js_re!(r"(?:(?<![\w./-])[\w./-]+:\d+:\d+\s+lint\/[a-z]+\/\w+|\bFound \d+ errors?\b)");

struct Signature {
    id: &'static str,
    label: &'static str,
    re: &'static LazyRe,
}

static TSC: LazyRe = js_re!(r"\berror TS\d{4}\b");
static TEST: LazyRe = js_re!(
    r"(?:Tests\s+[1-9]\d* failed|\([1-9]?\d* ?tests? \| [1-9]\d* failed\)|\bFAIL\s+[\w./-]*\.(?:test|spec)\.[jt]sx?\b|\b[1-9]\d* fail\b)"
);
static TRACEBACK: LazyRe = js_re!(r"Traceback \(most recent call last\):");
static NODE: LazyRe = js_re!(r"\n\s+at [^\n]*(?::\d+:\d+\)|\.(?:ts|tsx|js|mjs|cjs):\d+)");

static SIGNATURES: [Signature; 5] = [
    Signature {
        id: "tsc",
        label: "error TS####",
        re: &TSC,
    },
    Signature {
        id: "lint",
        label: "lint diagnostic",
        re: &LINT_RE,
    },
    Signature {
        id: "test",
        label: "N failing tests",
        re: &TEST,
    },
    Signature {
        id: "traceback",
        label: "python traceback",
        re: &TRACEBACK,
    },
    Signature {
        id: "node",
        label: "node stack frame",
        re: &NODE,
    },
];

static PIPED: LazyRe =
    js_re!(r"\|\s*(?:head|tail|grep|egrep|rg|wc|jq|sed|awk|tee|sort|uniq|cut)\b");
static EXIT_HEADER: LazyRe = js_re!(r"^Exit code (\d+)");
static EXIT_PREFIX: LazyRe = js_re!(r"^Exit code \d+\n?");
static CHAIN: LazyRe = js_re!(r";|&&|\|\||\n");
const ABSENCE_BINS: [&str; 9] = [
    "ls", "which", "type", "stat", "test", "find", "du", "wc", "file",
];
static ABSENT_TEXT: LazyRe = js_re!(r"No such file or directory|not found|does not exist", "i");
static ERROR_TEXT: LazyRe = js_re!(
    r"Traceback|error TS\d|SyntaxError|TypeError|KeyError|ValueError|AssertionError|Cannot find module|ERR_MODULE_NOT_FOUND|fatal:|ModuleNotFoundError|timed out|rate limit|<tool_use_error>|FAIL |error:"
);
pub static USER_REJECTED: LazyRe =
    js_re!(r"doesn't want to proceed with this tool use|doesn't want to take this action");
static GUARDED_TAIL: LazyRe = js_re!(r"2>\s*\/dev\/null");
const CHAINED_TAIL_MIN_CHARS: usize = 200;
static ENV_ASSIGNMENT: LazyRe = js_re!(r"^[A-Z_]+=");
static LEADING_PATH: LazyRe = js_re!(r"^.*\/");

/// What the output scanner found in one result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputScan {
    pub failed: bool,
    pub reason: Option<&'static str>,
}

/// Shell tools only: the first failure signature the output prints.
pub fn scan_output(tool: &str, text: &str, is_error: bool) -> OutputScan {
    let hit = if is_shell_tool(tool) {
        SIGNATURES.iter().find(|s| s.re.is_match(text))
    } else {
        None
    };
    OutputScan {
        failed: is_error || hit.is_some(),
        reason: hit.map(|s| s.id),
    }
}

pub fn is_piped(cmd: &str) -> bool {
    PIPED.is_match(cmd)
}

fn tail_of(cmd: &str) -> &str {
    let parts = CHAIN.split(cmd);
    text::trim(
        parts
            .into_iter()
            .rfind(|s| !text::trim(s).is_empty())
            .unwrap_or(cmd),
    )
}

fn first_word(segment: &str) -> String {
    let w = text::trim(segment)
        .split(text::is_js_whitespace)
        .filter(|s| !s.is_empty())
        .find(|w| !ENV_ASSIGNMENT.is_match(w))
        .unwrap_or("");
    LEADING_PATH.replace_first(w, |_| String::new())
}

/// Why a bash failure is benign, or `None` when it is a real failure.
pub fn benign_filter(cmd: &str, output: &str, cause: bool) -> Option<&'static str> {
    if cause {
        return None;
    }
    if USER_REJECTED.is_match(output) {
        return Some("user-rejected");
    }
    if !EXIT_HEADER.is_match(output) {
        return None;
    }
    if output.contains("no matches found") {
        return Some("glob-no-match");
    }
    let body_owned = EXIT_PREFIX.replace_first(output, |_| String::new());
    let body = text::trim(&body_owned);
    let tail = tail_of(cmd);
    let probes = ABSENCE_BINS.contains(&first_word(tail).as_str()) || GUARDED_TAIL.is_match(tail);
    if probes && (ABSENT_TEXT.is_match(output) || body.is_empty()) {
        return Some("absence-probe");
    }
    (CHAIN.is_match(cmd)
        && units::len(output) >= CHAINED_TAIL_MIN_CHARS
        && !ERROR_TEXT.is_match(body))
    .then_some("chained-tail")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FailKind {
    Flag,
    Scan,
}

struct FailMeta {
    kind: FailKind,
    label: String,
    piped: f64,
    tools: IndexMap<String, f64>,
}

fn label_for_reason(reason: &str) -> &str {
    SIGNATURES
        .iter()
        .find(|s| s.id == reason)
        .map_or(reason, |s| s.label)
}

/// The fail row a call is charged to, if any.
fn key_of(f: &CallFact) -> Option<(String, FailKind, String)> {
    if f.cause.is_some_and(|c| f.failed && spec(c).failing) {
        return None;
    }
    if f.err {
        return f.benign.is_none().then(|| {
            (
                format!("flag:{}", f.tool),
                FailKind::Flag,
                format!("{} errors", f.tool),
            )
        });
    }
    match (&f.fail_reason, f.failed) {
        (Some(r), true) => Some((
            format!("scan:{r}"),
            FailKind::Scan,
            label_for_reason(r).to_owned(),
        )),
        _ => None,
    }
}

fn line_of(f: &CallFact) -> &str {
    match &f.bash {
        Some(b) => &b.cmd,
        None => f.sig.as_deref().or(f.arg.as_deref()).unwrap_or(&f.tool),
    }
}

/// Chronological, then sid and seq.
pub fn compare_hits(a: &DrillHit, b: &DrillHit) -> Ordering {
    cmp_str(
        a.sample.ts.as_deref().unwrap_or(""),
        b.sample.ts.as_deref().unwrap_or(""),
    )
    .then_with(|| cmp_str(&a.sample.sid, &b.sample.sid))
    .then_with(|| asc(a.sample.seq, b.sample.seq))
}

#[derive(Debug, Clone, PartialEq)]
pub struct FailRow {
    pub key: String,
    pub label: String,
    pub r#where: String,
    pub rows: f64,
    pub sessions: f64,
    pub projects: f64,
    pub flagged: f64,
    pub note: Option<String>,
    pub detector: Option<String>,
    pub samples: Vec<SampleRef>,
}

impl FailRow {
    pub fn write(&self, w: &mut Writer) {
        w.begin_obj();
        w.key("key").text(&self.key);
        w.key("label").text(&self.label);
        w.key("where").text(&self.r#where);
        w.key("rows").num(self.rows);
        w.key("sessions").num(self.sessions);
        w.key("projects").num(self.projects);
        w.key("flagged").num(self.flagged);
        w.key("note").opt_text(self.note.as_deref());
        w.key("detector").opt_text(self.detector.as_deref());
        w.key("samples");
        write_samples(w, &self.samples);
        w.end_obj();
    }
}

/// The fail table: flagged rows and silent failures.
#[derive(Default)]
pub struct FailTable {
    drill: Option<DrillTarget>,
    tallies: IndexMap<String, Tally>,
    metas: IndexMap<String, FailMeta>,
    hits: Vec<DrillHit>,
    errored_bash: f64,
    suppressed: f64,
    claimed: f64,
}

impl FailTable {
    pub fn new(drill: Option<DrillTarget>) -> Self {
        Self {
            drill,
            ..Self::default()
        }
    }

    pub fn add(&mut self, fact: &Fact) {
        let Fact::Call(f) = fact else { return };
        if f.is_bash() && f.err {
            self.errored_bash += 1.0;
            if f.benign.is_some() {
                self.suppressed += 1.0;
            }
            if f.cause.is_some() {
                self.claimed += 1.0;
            }
        }
        let Some((key, kind, label)) = key_of(f) else {
            return;
        };
        let line = sample_line(line_of(f));
        self.tallies
            .entry(key.clone())
            .or_default()
            .add(f, Some(sample_of(f, line.clone())));
        let meta = self.metas.entry(key.clone()).or_insert_with(|| FailMeta {
            kind,
            label,
            piped: 0.0,
            tools: IndexMap::new(),
        });
        bump(&mut meta.tools, &f.tool, 1.0);
        if f.bash.as_ref().is_some_and(|b| b.piped) {
            meta.piped += 1.0;
        }
        if self
            .drill
            .as_ref()
            .is_some_and(|d| d.table == DrillTable::Fail && d.key == key)
        {
            self.hits.push(hit_of(f, line));
        }
    }

    pub fn finish(&self, ctx: &mut FinishCtx<'_>) -> Vec<FailRow> {
        let rows: Vec<FailRow> = self
            .tallies
            .iter()
            .filter_map(|(key, tally)| {
                let meta = self.metas.get(key)?;
                let scan = meta.kind == FailKind::Scan;
                let note = scan.then(|| {
                    if meta.piped > 0.0 {
                        format!(
                            "{} of {} pipe into head/tail/grep, so the exit code is the pipe's",
                            meta.piped, tally.rows
                        )
                    } else {
                        "the command printed a failure and still exited 0".to_owned()
                    }
                });
                let tool = top_key(&meta.tools);
                Some(FailRow {
                    key: key.clone(),
                    label: ctx.redact.line(&meta.label),
                    r#where: if scan { format!("{tool} exit 0") } else { tool },
                    rows: tally.rows,
                    sessions: tally.session_count(),
                    projects: tally.project_count(),
                    flagged: tally.flagged,
                    note: note.map(|n| ctx.redact.line(&n)),
                    detector: (scan && meta.piped > 0.0).then(|| PIPED_EXIT_CODE.to_owned()),
                    samples: samples_of(&tally.samples, ctx),
                })
            })
            .collect();
        let ranked = rank(
            rows,
            |a, b| {
                desc(a.sessions, b.sessions)
                    .then_with(|| desc(a.rows, b.rows))
                    .then_with(|| cmp_str(&a.key, &b.key))
            },
            ctx.limit,
        );
        if ranked.dropped > 0 {
            ctx.caveat(format!("fail table: {} rows not shown", ranked.dropped));
        }
        if self.suppressed > 0.0 {
            let pct = round_to(share(self.suppressed, self.errored_bash) * 100.0, 1);
            ctx.caveat(format!(
                "{} of {} errored bash rows ({}%) are suppressed as benign, at a precision of 78% measured before the cause catalog took {} rows (zsh glob aborts among them) out of the benign set",
                self.suppressed,
                self.errored_bash,
                crate::js::num::to_js_string(pct),
                self.claimed
            ));
        }
        ranked.rows
    }

    pub fn drill(&self, ctx: &FinishCtx<'_>) -> Vec<DrillHit> {
        let mut hits = redact_hits(&self.hits, ctx.redact);
        hits.sort_by(compare_hits);
        hits
    }
}
