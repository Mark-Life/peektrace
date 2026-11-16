//! The cause table: fixes, ranking, the dirs and covers a rule check
//! reads, and drills.
#![allow(clippy::float_cmp, reason = "counts are whole numbers")]

mod common;

use common::causes::*;
use common::rules::cover;
use peektrace_core::Agent;
use peektrace_core::stats::accumulate::DrillHit;
use peektrace_core::stats::detectors::cause_row::CauseRow;
use peektrace_core::stats::detectors::cells::call_fix_of;
use peektrace_core::stats::detectors::retries::CauseTable;
use peektrace_core::stats::detectors::rules::{
    CauseCounts, CodexCover, CoverSince, RuleCover, Split,
};
use peektrace_core::stats::redact::Redactor;
use peektrace_core::stats::schema::{CallFact, CauseId, Kind};

macro_rules! facts {
    ($($e:expr),* $(,)?) => { vec![$(In::from($e)),*] };
}

#[test]
fn fd_and_nvm_fixes() {
    let fd = call_fix_of(CauseId::CommandNotFound, Some("fd")).unwrap_or_default();
    assert!(fd.contains("mkdir -p ~/.local/bin && ln -s $(command -v fdfind) ~/.local/bin/fd"));
    assert!(fd.contains("put ~/.local/bin on PATH in ~/.zshenv"));
    let nvm = call_fix_of(CauseId::CommandNotFound, Some("nvm")).unwrap_or_default();
    assert!(nvm.contains("source $NVM_DIR/nvm.sh"));
    assert!(!nvm.contains("put nvm on PATH"));
}

#[test]
fn rows_rank_by_retry_sessions_then_sessions_then_pairs() {
    let rows = rows_of(facts![
        with(err(bash_at(0, 0.0, "pkill -f srv; run")), |f| {
            f.cause = Some(CauseId::PkillSelfKill);
        }),
        glob(1),
        ok(2),
    ]);
    let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, [GLOB, "cause:pkill-self-kill"]);
}

fn hits_since(t: &CauseTable<'_>, cover: &RuleCover, after_day: &str) -> (Split, Split) {
    t.since(
        GLOB,
        &[CoverSince {
            after_day: after_day.to_owned(),
            cover,
        }],
    )
}

#[test]
fn the_dirs_a_rule_check_reads_are_the_hits_cwds() {
    let t = table_of(
        facts![
            session("s1", |f| f.cwd = s("/work/b")),
            session("s0", |f| f.cwd = s("/work/a")),
            glob(0),
        ],
        None,
    );
    assert_eq!(t.project_dirs(), ["/work/b"]);
}

#[test]
fn an_agent_stored_under_one_project_reads_its_repo() {
    let sub = |f: CallFact| {
        with(f, |f| {
            f.base.sid = "agent".into();
            f.base.kind = Kind::Subagent;
        })
    };
    let mut t = table_of(
        facts![
            session("lead", |f| f.cwd = s("/work/lead")),
            with(glob(0), |f| f.base.sid = "lead".into()),
            session("agent", |f| {
                f.base.kind = Kind::Subagent;
                f.cwd = s("/work/other");
            }),
            sub(glob(1)),
        ],
        None,
    );
    assert_eq!(t.project_dirs(), ["/work/lead", "/work/other"]);
    finish(&mut t);
    let (hits, _) = hits_since(&t, &cover(Agent::Claude, Some("/work/other")), "");
    assert_eq!(hits, split(0.0, 1.0));
}

#[test]
fn a_call_without_its_session_row_falls_back_to_its_project_dir() {
    let t = table_of(
        facts![
            session("s0", |f| f.cwd = s("/work/a")),
            with(glob(0), |f| f.base.sid = "gone".into()),
        ],
        None,
    );
    assert_eq!(t.project_dirs(), ["/work/a"]);
}

#[test]
fn hits_since_a_day_count_whole_days_after_it() {
    let mut t = table_of(
        facts![
            with(glob(0), |f| f.ts = s("2026-08-01T10:00:00.000Z")),
            with(glob(1), |f| {
                f.ts = s("2026-08-02T10:00:00.000Z");
                f.base.sid = "s2".into();
            }),
            with(ok(2), |f| {
                f.ts = s("2026-08-02T11:00:00.000Z");
                f.base.sid = "s3".into();
            }),
        ],
        None,
    );
    finish(&mut t);
    let (hits, calls) = hits_since(&t, &cover(Agent::Claude, None), "2026-08-01");
    assert_eq!(hits, split(1.0, 0.0));
    assert_eq!(calls, split(2.0, 0.0));
}

#[test]
fn a_file_counts_only_the_calls_it_reaches() {
    let at_project = |sid: &str, project: &str, agent: Agent, cwd: &str, seq: u32| {
        let (sid, project, cwd) = (sid.to_owned(), project.to_owned(), cwd.to_owned());
        facts![
            session(&sid, |f| {
                f.base.project.clone_from(&project);
                f.base.agent = agent;
                f.cwd = Some(cwd);
            }),
            with(glob(seq), |f| {
                f.base.sid = sid;
                f.base.project = project;
                f.base.agent = agent;
            }),
        ]
    };
    let mut all = at_project("a", "pa", Agent::Claude, "/work/a/sub", 0);
    all.extend(at_project("b", "pb", Agent::Claude, "/work/b", 1));
    all.extend(at_project("c", "pc", Agent::Codex, "/x", 2));
    let mut t = table_of(all, None);
    finish(&mut t);
    let hits = |agent: Agent, dir: Option<&str>| hits_since(&t, &cover(agent, dir), "").0.main;
    assert_eq!(hits(Agent::Claude, None), 2.0);
    assert_eq!(hits(Agent::Codex, None), 1.0);
    assert_eq!(hits(Agent::Claude, Some("/work/a")), 1.0);
    assert_eq!(hits(Agent::Claude, Some("/work")), 2.0);
    assert_eq!(hits(Agent::Claude, Some("/work/ab")), 0.0);
    assert_eq!(hits(Agent::Codex, Some("/work")), 0.0);
    assert_eq!(hits(Agent::Codex, Some("/x")), 1.0);
}

#[test]
fn a_codex_project_file_reaches_its_repo_but_a_nested_ones() {
    let mut all = Vec::new();
    for (sid, cwd, seq) in [
        ("a", "/r", 0),
        ("b", "/r/pkg/deep", 1),
        ("c", "/r/vendor/inner/src", 2),
    ] {
        all.push(
            session(sid, |f| {
                f.base.project = sid.into();
                f.base.agent = Agent::Codex;
                f.cwd = s(cwd);
            })
            .into(),
        );
        all.push(
            with(glob(seq), |f| {
                f.base.sid = sid.into();
                f.base.project = sid.into();
                f.base.agent = Agent::Codex;
            })
            .into(),
        );
    }
    let mut t = table_of(all, None);
    finish(&mut t);
    let hits = |exact: bool, stops: &[&str]| {
        let c = RuleCover {
            codex: Some(CodexCover {
                exact,
                stops: stops.iter().map(|x| (*x).to_owned()).collect(),
            }),
            ..cover(Agent::Codex, Some("/r"))
        };
        hits_since(&t, &c, "").0.main
    };
    assert_eq!(hits(false, &[]), 3.0);
    assert_eq!(hits(false, &["/r/vendor/inner"]), 2.0);
    assert_eq!(hits(true, &[]), 1.0);
}

#[test]
fn a_sourced_global_file_reaches_only_its_sources_sessions() {
    let mut t = table_of(
        facts![
            session("d", |f| f.cwd = s("/w")),
            with(glob(0), |f| f.base.sid = "d".into()),
            session("w", |f| {
                f.cwd = s("/w");
                f.source = s("work");
            }),
            with(glob(1), |f| f.base.sid = "w".into()),
            with(glob(2), |f| f.base.sid = "w".into()),
        ],
        None,
    );
    finish(&mut t);
    let hits = |id: &str, is_default: bool| {
        let c = RuleCover {
            source: Some((id.to_owned(), is_default)),
            ..cover(Agent::Claude, None)
        };
        hits_since(&t, &c, "").0.main
    };
    assert_eq!(hits("work", false), 2.0);
    assert_eq!(hits("claude", true), 1.0);
}

fn drill_of(facts: Vec<In>) -> (Vec<CauseRow>, Vec<DrillHit>) {
    let mut t = table_of(facts, Some(GLOB));
    let rows = finish(&mut t);
    let hits = t.drill(&Redactor::new(true));
    (rows, hits)
}

#[test]
fn drill_returns_pairs_first_with_the_retry_line() {
    let (_, hits) = drill_of(facts![
        glob(0),
        ok(1),
        with(glob(2), |f| f.base.sid = "s2".into())
    ]);
    let got: Vec<(&str, f64)> = hits
        .iter()
        .map(|h| (h.sample.sid.as_str(), h.sample.seq))
        .collect();
    assert_eq!(got, [("s1", 0.0), ("s2", 2.0)]);
    assert!(!hits[0].sample.line.contains(" → "));
    assert_eq!(
        hits[0].pair_line.as_deref(),
        Some(ok(1).bash.expect("bash").cmd.as_str())
    );
    assert_eq!(hits[1].pair_line, None);
}

#[test]
fn links_carry_the_transcripts_own_call_ids() {
    let (rows, hits) = drill_of(facts![
        with(glob(0), |f| f.tool_use_id = s("t0")),
        with(ok(1), |f| f.tool_use_id = s("t1")),
    ]);
    assert_eq!(rows[0].samples[0].tool_use_id.as_deref(), Some("t0"));
    assert_eq!(
        rows[0].pair_samples[0].retry.tool_use_id.as_deref(),
        Some("t1")
    );
    assert_eq!(
        (
            hits[0].sample.tool_use_id.as_deref(),
            hits[0].pair_tool_use_id.as_deref()
        ),
        (Some("t0"), Some("t1"))
    );
}
