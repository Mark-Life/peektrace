//! Per-session markers: the named cause, retry, silent failure, ceiling and
//! repeat marks a transcript view draws on its calls.

use indexmap::IndexMap;

use super::detectors::causes::spec;
use super::detectors::cells::call_fix_of;
use super::detectors::pairing::{Pairer, RetryPair};
use super::detectors::retries::FIX_MAX;
use super::rank::{SAMPLE_LINE_MAX, asc, clip, cmp_str};
use super::redact::Redactor;
use super::schema::{CallFact, Fact};
use crate::js::json_out::Writer;
use crate::js::num::{js_round, to_js_string};
use crate::js::time::LocalZone;

const SIG_MAX: usize = 80;
const CAUSE_MARK_MAX: usize = SAMPLE_LINE_MAX + FIX_MAX;
const MIN_REPEATS: usize = 2;

/// One mark on one call.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionMarker {
    pub seq: f64,
    pub tool_use_id: Option<String>,
    pub detector: String,
    pub label: String,
}

impl SessionMarker {
    pub fn write(&self, w: &mut Writer) {
        w.begin_obj();
        w.key("seq").num(self.seq);
        match &self.tool_use_id {
            Some(t) => w.key("toolUseId").text(t),
            None => w.key("toolUseId").null(),
        }
        w.key("detector").str(&self.detector);
        w.key("label").text(&self.label);
        w.end_obj();
    }
}

fn mark(f: &CallFact, detector: &str, label: String) -> SessionMarker {
    SessionMarker {
        seq: f.seq,
        tool_use_id: f.tool_use_id.clone(),
        detector: detector.to_owned(),
        label,
    }
}

fn cause_marker(f: &CallFact) -> Option<SessionMarker> {
    let cause = f.cause?;
    let culprit = f
        .cause_arg
        .as_ref()
        .map_or_else(String::new, |a| format!(" ({a})"));
    let label = spec(cause).label;
    Some(mark(
        f,
        cause.as_str(),
        match call_fix_of(cause, f.cause_arg.as_deref()) {
            Some(fix) => format!("{label}{culprit}: {fix}"),
            None => format!("{label}{culprit}"),
        },
    ))
}

fn ago_of(gap: f64) -> String {
    let sec = js_round(gap);
    if sec < 60.0 {
        format!("{}s", to_js_string(sec))
    } else {
        format!("{}m", to_js_string(js_round(sec / 60.0)))
    }
}

fn retry_marker(p: &RetryPair) -> SessionMarker {
    let what = if p.read_back {
        "read-back of"
    } else {
        "retry of"
    };
    let tool = &p.failed.tool;
    let which = p.gap_sec.map_or_else(
        || format!("an earlier {tool} call"),
        |g| format!("the {tool} call {} earlier", ago_of(g)),
    );
    let diff = p
        .diff
        .as_ref()
        .map_or_else(String::new, |d| format!(": {} → {}", d.from, d.to));
    mark(&p.retry, "retry", format!("{what} {which}{diff}"))
}

fn silent_marker(f: &CallFact) -> Option<SessionMarker> {
    let reason = f.fail_reason.as_deref().filter(|_| !f.err && f.failed)?;
    Some(if f.bash.as_ref().is_some_and(|b| b.piped) {
        mark(
            f,
            "piped-exit-code",
            format!("printed a failure ({reason}) and exited 0: the pipe owns the exit code"),
        )
    } else {
        mark(
            f,
            "silent-fail",
            format!("printed a failure ({reason}) and exited 0"),
        )
    })
}

fn capped_marker(f: &CallFact) -> Option<SessionMarker> {
    f.bash.as_ref().filter(|b| b.capped).map(|_| {
        mark(
            f,
            "capped",
            "returned at a harness ceiling: the duration is a floor".to_owned(),
        )
    })
}

/// Collects every marker for one session id.
pub struct MarkerCollector<'a> {
    sid: String,
    redact: &'a Redactor,
    marks: Vec<SessionMarker>,
    by_sig: IndexMap<String, Vec<CallFact>>,
    pairer: Pairer<'a>,
}

impl<'a> MarkerCollector<'a> {
    pub fn new(sid: &str, redact: &'a Redactor, local: &'a dyn LocalZone) -> Self {
        Self {
            sid: sid.to_owned(),
            redact,
            marks: Vec::new(),
            by_sig: IndexMap::new(),
            pairer: Pairer::new(local),
        }
    }

    fn on_pairs(&mut self, pairs: &[RetryPair]) {
        for p in pairs {
            let m = retry_marker(p);
            self.marks.push(SessionMarker {
                label: self.redact.line(&m.label),
                ..m
            });
        }
    }

    pub fn add(&mut self, fact: &Fact) {
        let Fact::Call(f) = fact else { return };
        if f.base.sid != self.sid {
            return;
        }
        let pairs = self.pairer.step(f);
        self.on_pairs(&pairs);
        if f.benign.is_some() {
            return;
        }
        if let Some(m) = cause_marker(f) {
            self.marks.push(SessionMarker {
                label: self.redact.line_max(&m.label, CAUSE_MARK_MAX),
                ..m
            });
        }
        for m in [silent_marker(f), capped_marker(f)].into_iter().flatten() {
            self.marks.push(SessionMarker {
                label: self.redact.line(&m.label),
                ..m
            });
        }
        if let Some(sig) = f.sig.as_ref().filter(|_| f.failed || f.err) {
            self.by_sig.entry(sig.clone()).or_default().push(f.clone());
        }
    }

    /// Every marker for the session, in transcript order.
    pub fn finish(mut self) -> Vec<SessionMarker> {
        let pairs = self.pairer.flush();
        self.on_pairs(&pairs);
        let mut out = self.marks;
        for (sig, facts) in &self.by_sig {
            if facts.len() < MIN_REPEATS {
                continue;
            }
            let label = self.redact.line(&clip(sig, SIG_MAX));
            for (i, f) in facts.iter().enumerate().skip(1) {
                out.push(mark(
                    f,
                    "repeat",
                    format!(
                        "repeat {} of {} in this session: {label}",
                        i + 1,
                        facts.len()
                    ),
                ));
            }
        }
        out.sort_by(|a, b| asc(a.seq, b.seq).then_with(|| cmp_str(&a.detector, &b.detector)));
        out
    }
}
