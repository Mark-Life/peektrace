//! Waiting: how a command parks (loop, watcher, retry, bare delay) and the
//! panel that sums it.

use std::cmp::Ordering;

use indexmap::IndexMap;

use crate::js::json_out::Writer;
use crate::js::num::parse_number;
use crate::js::regex::LazyRe;
use crate::js::text;
use crate::js_re;
use crate::stats::accumulate::{
    DrillHit, DrillTable, DrillTarget, FinishCtx, SampleRef, Tally, hit_of, redact_hits,
    sample_line, sample_of, samples_of, write_samples,
};
use crate::stats::rank::{asc, cmp_str, desc, median, percentile, round, share};
use crate::stats::schema::{Fact, Job, WaitShape};

const P95: f64 = 0.95;

/// Longest first, then chronological.
pub fn compare_drill_hits(a: &DrillHit, b: &DrillHit) -> Ordering {
    desc(a.dur_sec.unwrap_or(0.0), b.dur_sec.unwrap_or(0.0))
        .then_with(|| cmp_str(a.sample.ts.as_deref().unwrap_or(""), b.sample.ts.as_deref().unwrap_or("")))
        .then_with(|| cmp_str(&a.sample.sid, &b.sample.sid))
        .then_with(|| asc(a.sample.seq, b.sample.seq))
}

static HEREDOC_OPEN: LazyRe = js_re!(r#"<<-?\s*['"]?([A-Za-z_][A-Za-z0-9_]*)['"]?"#);
static COMMENT_LINE: LazyRe = js_re!(r"^\s*#");
static SINGLE_QUOTED: LazyRe = js_re!(r"'[^']*'", "g");
static DOUBLE_QUOTED: LazyRe = js_re!(r#""[^"]*""#, "g");

/// Drop every heredoc body, keeping the line that opens it.
pub fn strip_heredoc_bodies(cmd: &str) -> String {
    if !cmd.contains("<<") {
        return cmd.to_owned();
    }
    let mut out: Vec<&str> = Vec::new();
    let mut tag: Option<String> = None;
    for line in cmd.split('\n') {
        if let Some(t) = &tag {
            if text::trim(line) == t {
                tag = None;
            }
            continue;
        }
        out.push(line);
        if let Some(open) = HEREDOC_OPEN.captures(line) {
            tag = open.get(1).map(str::to_owned);
        }
    }
    out.join("\n")
}

pub fn strip_comments(cmd: &str) -> String {
    if !cmd.contains('#') {
        return cmd.to_owned();
    }
    cmd.split('\n')
        .filter(|l| !COMMENT_LINE.is_match(l))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn mask_quoted(cmd: &str) -> String {
    let s = SINGLE_QUOTED.replace_all_str(cmd, "''");
    DOUBLE_QUOTED.replace_all_str(&s, "\"\"")
}

macro_rules! at_cmd {
    () => {
        r"(?:^|[;&|(\n]|\bdo\b|\bthen\b)\s*"
    };
}

static LOOP_HEAD: LazyRe = js_re!(concat!(at_cmd!(), r"(?:while|until)\s*(?=[\s[!])"));
static FOR_HEAD: LazyRe = js_re!(concat!(at_cmd!(), r"for\s+\w+\s+in\b"));
static LIST_ITERATION: LazyRe = js_re!(r"\b(?:while|until)\s+(?:read\b|IFS=)");
static DELAY_AT_CMD: LazyRe = js_re!(concat!(at_cmd!(), r"(?:\S*/)?(?:sleep|usleep)\s+\.?\d"));
static DELAY_ANYWHERE: LazyRe = js_re!(
    r"(?:\bsleep\s+\.?\d|\busleep\s+\d|\bread\s+-t\s*\d|select\s+undef\s*,\s*undef\s*,\s*undef|\btime\.sleep\s*\(\s*\.?\d)"
);
static LONG_INLINE_DELAY: LazyRe =
    js_re!(r"(?:\btime\.sleep\s*\(\s*\d{2,}|select\s+undef\s*,\s*undef\s*,\s*undef\s*,\s*\d{2,})");
static INTERPRETER: LazyRe = js_re!(r"\b(?:python3?|perl|node|bun|ruby)\s+-[ce]\b");
static LOOP_ANY: LazyRe = js_re!(r"\b(?:while|until|for)\b");
static WATCHER: LazyRe = js_re!(concat!(
    at_cmd!(),
    r"(?:watch|wait-on|wait-for-it(?:\.sh)?|dockerize)\b|\bgh\s+run\s+watch\b|\btail\s+(?:-[a-zA-Z]*f|--follow)\b"
));
static CURL_RETRY: LazyRe = js_re!(r"\bcurl\b[^\n]*--retry");

/// The waiting shape of a command, or `None`. First match wins.
pub fn wait_shape(cmd: &str) -> Option<WaitShape> {
    let text = strip_comments(&strip_heredoc_bodies(cmd));
    let st = mask_quoted(&text);
    let shell_loop = (LOOP_HEAD.is_match(&st) || FOR_HEAD.is_match(&st)) && !LIST_ITERATION.is_match(&st);
    let inline_loop = INTERPRETER.is_match(&st) && LOOP_ANY.is_match(&text) && LONG_INLINE_DELAY.is_match(&text);
    if (shell_loop && DELAY_ANYWHERE.is_match(&text)) || inline_loop {
        return Some(WaitShape::LoopPoll);
    }
    if WATCHER.is_match(&st) {
        return Some(WaitShape::Watcher);
    }
    if CURL_RETRY.is_match(&st) {
        return Some(WaitShape::CurlRetry);
    }
    if DELAY_AT_CMD.is_match(&st) || (INTERPRETER.is_match(&st) && LONG_INLINE_DELAY.is_match(&text)) {
        return Some(WaitShape::BareDelay);
    }
    None
}

pub fn is_waiting(cmd: &str) -> bool {
    wait_shape(cmd).is_some()
}

static SLEEP_LITERAL: LazyRe = js_re!(r"\b(?:\S*\/)?sleep\s+(\.?\d[\d.]*)", "g");
static PY_SLEEP_LITERAL: LazyRe = js_re!(r"\btime\.sleep\s*\(\s*(\.?\d[\d.]*)", "g");
static PERL_SLEEP_LITERAL: LazyRe = js_re!(r"select\s+undef\s*,\s*undef\s*,\s*undef\s*,\s*(\.?\d[\d.]*)", "g");

/// The literal seconds a line asked to sleep for.
pub fn declared_delay_sec(cmd: &str) -> f64 {
    let text = strip_comments(&strip_heredoc_bodies(cmd));
    [&SLEEP_LITERAL, &PY_SLEEP_LITERAL, &PERL_SLEEP_LITERAL]
        .iter()
        .flat_map(|re| re.captures_all(&text))
        .map(|c| {
            let n = parse_number(c.get(1).unwrap_or(""));
            if n.is_nan() || n == 0.0 { 0.0 } else { n }
        })
        .sum()
}

#[derive(Debug, Clone, PartialEq)]
pub struct WaitShapeRow {
    pub key: WaitShape,
    pub calls: f64,
    pub total_sec: f64,
    pub share: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WaitPanel {
    pub calls: f64,
    pub total_sec: f64,
    pub share: f64,
    pub sessions: f64,
    pub projects: f64,
    pub median_sec: f64,
    pub p95_sec: f64,
    pub by_shape: Vec<WaitShapeRow>,
    pub bg_runs: f64,
    pub bg_sec: f64,
    pub bg_declared_sec: f64,
    pub samples: Vec<SampleRef>,
}

impl WaitPanel {
    pub fn write(&self, w: &mut Writer) {
        w.begin_obj();
        for (k, v) in [
            ("calls", self.calls),
            ("totalSec", self.total_sec),
            ("share", self.share),
            ("sessions", self.sessions),
            ("projects", self.projects),
            ("medianSec", self.median_sec),
            ("p95Sec", self.p95_sec),
        ] {
            w.key(k).num(v);
        }
        w.key("byShape").begin_arr();
        for r in &self.by_shape {
            w.elem().begin_obj();
            w.key("key").str(r.key.as_str());
            w.key("calls").num(r.calls);
            w.key("totalSec").num(r.total_sec);
            w.key("share").num(r.share);
            w.end_obj();
        }
        w.end_arr();
        w.key("bgRuns").num(self.bg_runs);
        w.key("bgSec").num(self.bg_sec);
        w.key("bgDeclaredSec").num(self.bg_declared_sec);
        w.key("samples");
        write_samples(w, &self.samples);
        w.end_obj();
    }
}

const ALL: &str = "all";

#[derive(Default)]
pub struct WaitPanelSink {
    wanted: Option<String>,
    tallies: IndexMap<String, Tally>,
    hits: Vec<DrillHit>,
    bg_runs: f64,
    bg_sec: f64,
    bg_declared_sec: f64,
}

impl WaitPanelSink {
    pub fn new(drill: Option<&DrillTarget>) -> Self {
        Self {
            wanted: drill
                .filter(|d| d.table == DrillTable::Wait)
                .map(|d| d.key.clone()),
            ..Self::default()
        }
    }

    pub fn add(&mut self, fact: &Fact) {
        let Fact::Call(f) = fact else { return };
        let Some(b) = f.bash.as_deref() else { return };
        if b.job != Job::WaitPoll {
            return;
        }
        let line = sample_line(&b.cmd);
        let sample = sample_of(f, line.clone());
        if let Some(wait) = b.wait {
            self.tallies
                .entry(ALL.to_owned())
                .or_default()
                .add(f, Some(sample.clone()));
            self.tallies
                .entry(wait.as_str().to_owned())
                .or_default()
                .add(f, Some(sample));
        } else {
            self.tallies.entry(ALL.to_owned()).or_default().add(f, Some(sample));
        }
        if b.bg {
            self.bg_runs += 1.0;
            self.bg_sec += f.dur_sec.unwrap_or(0.0);
            self.bg_declared_sec += b.declared_delay_sec;
        }
        let matches = self.wanted.as_deref().is_some_and(|w| {
            w == ALL || w == Job::WaitPoll.as_str() || b.wait.is_some_and(|s| s.as_str() == w)
        });
        if matches {
            self.hits.push(hit_of(f, line));
        }
    }

    pub fn finish(&self, ctx: &FinishCtx<'_>) -> WaitPanel {
        let empty = Tally::default();
        let get = |k: &str| self.tallies.get(k).unwrap_or(&empty);
        let total = get(ALL);
        let mut by_shape: Vec<WaitShapeRow> = WaitShape::ALL
            .iter()
            .map(|&key| {
                let t = get(key.as_str());
                WaitShapeRow {
                    key,
                    calls: t.rows,
                    total_sec: round(t.total_sec),
                    share: share(t.total_sec, ctx.corpus.bash_sec),
                }
            })
            .collect();
        by_shape.sort_by(|a, b| desc(a.total_sec, b.total_sec).then_with(|| cmp_str(a.key.as_str(), b.key.as_str())));
        WaitPanel {
            calls: total.rows,
            total_sec: round(total.total_sec),
            share: share(total.total_sec, ctx.corpus.bash_sec),
            sessions: total.session_count(),
            projects: total.project_count(),
            median_sec: round(median(&total.durations)),
            p95_sec: round(percentile(&total.durations, P95)),
            by_shape,
            bg_runs: self.bg_runs,
            bg_sec: round(self.bg_sec),
            bg_declared_sec: round(self.bg_declared_sec),
            samples: samples_of(&total.samples, ctx),
        }
    }

    pub fn drill(&self, ctx: &FinishCtx<'_>) -> Vec<DrillHit> {
        let mut hits = redact_hits(&self.hits, ctx.redact);
        hits.sort_by(compare_drill_hits);
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes() {
        assert_eq!(wait_shape("sleep 30"), Some(WaitShape::BareDelay));
        assert_eq!(
            wait_shape("until grep -q DONE f; do sleep 2; done"),
            Some(WaitShape::LoopPoll)
        );
        assert_eq!(wait_shape("tail -f log"), Some(WaitShape::Watcher));
        assert_eq!(wait_shape("echo 'sleep 5'"), None);
        assert!((declared_delay_sec("sleep 2; sleep 1.5") - 3.5).abs() < 1e-9);
    }
}
