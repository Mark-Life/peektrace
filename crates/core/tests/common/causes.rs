//! Builders for the retry-pair and cause-table tests.

use peektrace_core::js::time::{Utc, to_iso_string};
use peektrace_core::stats::accumulate::{CorpusSummary, DrillTable, DrillTarget};
use peektrace_core::stats::detectors::cause_row::CauseRow;
use peektrace_core::stats::detectors::retries::CauseTable;
use peektrace_core::stats::detectors::rules::Split;
use peektrace_core::stats::redact::Redactor;
use peektrace_core::stats::schema::{CallFact, CauseId, Fact, SessionFact};

use super::{bash_fact, bash_mut, ctx_of, session_fact, tool_fact, window};

pub const GLOB: &str = "cause:zsh-glob-flag";
pub const START_MS: i64 = 1_785_578_400_000;

pub fn at(sec: f64) -> String {
    #[allow(clippy::cast_possible_truncation, reason = "whole ms")]
    let ms = (sec * 1000.0) as i64;
    to_iso_string(START_MS + ms)
}

pub fn with(mut f: CallFact, edit: impl FnOnce(&mut CallFact)) -> CallFact {
    edit(&mut f);
    f
}

#[allow(clippy::unnecessary_wraps, reason = "fills Option fields")]
pub fn s(v: &str) -> Option<String> {
    Some(v.to_owned())
}

pub fn glob(seq: u32) -> CallFact {
    let n = f64::from(seq);
    with(bash_fact("grep -rn foo --include=*.ts src | head"), |f| {
        f.seq = n;
        f.ts = s(&at(n * 5.0));
        f.req = Some(format!("r{seq}"));
        f.ctx_tok = Some(1000.0 * (n + 1.0));
        f.failed = true;
        f.cause = Some(CauseId::ZshGlobFlag);
        f.cause_arg = s("--include=*.ts");
        f.focus = s("grep -rn foo --include=*.ts src");
    })
}

pub fn ok(seq: u32) -> CallFact {
    let n = f64::from(seq);
    with(bash_fact("grep -rn foo --include='*.ts' src | head"), |f| {
        f.seq = n;
        f.ts = s(&at(n * 5.0));
        f.req = Some(format!("r{seq}"));
        f.ctx_tok = Some(1000.0 * (n + 1.0));
    })
}

/// A bash fact at `at(sec)` with request `r<seq>`.
pub fn bash_at(seq: u32, sec: f64, cmd: &str) -> CallFact {
    with(bash_fact(cmd), |f| {
        f.seq = f64::from(seq);
        f.ts = s(&at(sec));
        f.req = Some(format!("r{seq}"));
    })
}

pub fn tool_at(seq: u32, sec: f64, tool: &str) -> CallFact {
    with(tool_fact(), |f| {
        f.seq = f64::from(seq);
        f.ts = s(&at(sec));
        tool.clone_into(&mut f.tool);
    })
}

pub fn set_cmd(f: &mut CallFact, cmd: &str) {
    let b = bash_mut(f);
    cmd.clone_into(&mut b.cmd);
    b.ran = peektrace_core::stats::shell_lex::ran_bins(cmd);
}

pub fn err(mut f: CallFact) -> CallFact {
    f.err = true;
    f.failed = true;
    f
}

pub fn session(sid: &str, edit: impl FnOnce(&mut SessionFact)) -> Fact {
    let mut f = session_fact();
    sid.clone_into(&mut f.base.sid);
    edit(&mut f);
    Fact::Session(f)
}

pub enum In {
    F(Fact),
}

impl From<CallFact> for In {
    fn from(f: CallFact) -> Self {
        Self::F(Fact::Call(f))
    }
}

impl From<Fact> for In {
    fn from(f: Fact) -> Self {
        Self::F(f)
    }
}

pub fn table_of(facts: Vec<In>, drill: Option<&str>) -> CauseTable<'static> {
    let target = drill.map(|k| DrillTarget {
        table: DrillTable::Cause,
        key: k.to_owned(),
    });
    let mut t = CauseTable::new(target.as_ref(), &Utc);
    for In::F(f) in facts {
        t.add(&f);
    }
    t
}

pub fn finish(t: &mut CauseTable<'_>) -> Vec<CauseRow> {
    let (corpus, window, redact) = (CorpusSummary::default(), window(), Redactor::new(true));
    t.finish(&mut ctx_of(&corpus, &window, &redact))
}

pub fn rows_of(facts: Vec<In>) -> Vec<CauseRow> {
    finish(&mut table_of(facts, None))
}

pub fn row<'a>(rows: &'a [CauseRow], key: &str) -> Option<&'a CauseRow> {
    rows.iter().find(|r| r.key == key)
}

pub fn pairs(rows: &[CauseRow], key: &str) -> Option<f64> {
    row(rows, key).map(|r| r.pairs)
}

pub fn diffs(rows: &[CauseRow], key: &str) -> Option<Vec<(String, String, f64)>> {
    row(rows, key).map(|r| r.diffs.clone())
}

pub fn diff(from: &str, to: &str) -> Vec<(String, String, f64)> {
    vec![(from.to_owned(), to.to_owned(), 1.0)]
}

pub fn split(main: f64, nested: f64) -> Split {
    Split { main, nested }
}
