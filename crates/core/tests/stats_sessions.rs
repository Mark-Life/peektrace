//! Session counts: a main session and the subagent and workflow-agent runs
//! under it count once, in the corpus totals and in every per-cause spread.
#![allow(clippy::float_cmp, reason = "counts are whole numbers")]

mod common;

use common::causes::*;
use common::{bash_fact, session_fact};
use peektrace_core::stats::accumulate::{CorpusTotals, Tally};
use peektrace_core::stats::schema::{CallFact, Fact, Kind};

macro_rules! facts {
    ($($e:expr),* $(,)?) => { vec![$(In::from($e)),*] };
}

fn under(parent: &str, sid: &str, kind: Kind, mut f: CallFact) -> CallFact {
    sid.clone_into(&mut f.base.sid);
    f.base.kind = kind;
    f.base.parent = Some(parent.to_owned());
    f
}

fn transcript(sid: &str, kind: Kind, parent: Option<&str>) -> Fact {
    let mut f = session_fact();
    sid.clone_into(&mut f.base.sid);
    f.base.kind = kind;
    f.base.parent = parent.map(str::to_owned);
    Fact::Session(f)
}

#[test]
fn the_corpus_counts_main_sessions_apart_from_the_runs_under_them() {
    let mut totals = CorpusTotals::default();
    for f in [
        transcript("m1", Kind::Main, None),
        transcript("a1", Kind::Subagent, Some("m1")),
        transcript("w1", Kind::WorkflowAgent, Some("m1")),
        transcript("w2", Kind::WorkflowAgent, Some("m1")),
        transcript("m2", Kind::Main, None),
    ] {
        totals.add(&f);
    }
    let c = totals.finish();
    assert_eq!((c.transcripts, c.sessions), (5.0, 2.0));
    assert_eq!(c.split(), (2.0, 3.0));
}

#[test]
fn a_session_whose_main_transcript_is_outside_the_window_still_counts() {
    let mut totals = CorpusTotals::default();
    totals.add(&transcript("a1", Kind::Subagent, Some("m0")));
    assert_eq!(totals.finish().split(), (1.0, 1.0));
}

#[test]
fn a_tally_counts_the_root_session_once() {
    let mut t = Tally::default();
    let main = bash_fact("ls");
    let sid = main.base.sid.clone();
    t.add(&main, None);
    t.add(&under(&sid, "a1", Kind::Subagent, bash_fact("ls")), None);
    t.add(
        &under(&sid, "w1", Kind::WorkflowAgent, bash_fact("ls")),
        None,
    );
    assert_eq!((t.rows, t.session_count()), (3.0, 1.0));
}

#[test]
fn a_cause_hit_in_main_and_its_subagent_spans_one_session() {
    let sid = glob(0).base.sid;
    let nested = |f| under(&sid, "agent-1", Kind::Subagent, f);
    let rows = rows_of(facts![glob(0), ok(1), nested(glob(0)), nested(ok(1))]);
    let r = row(&rows, GLOB).expect("row");
    assert_eq!(r.hits_by, split(1.0, 1.0));
    assert_eq!((r.sessions, r.retry_sessions), (1.0, 1.0));
}
