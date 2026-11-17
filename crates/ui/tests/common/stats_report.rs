//! The inspector's stats test fixtures (`apps/inspector/test/stats-format.test.tsx`,
//! `stats-report-fixture.ts`, `session-chips.test.tsx`), as core rows.

#![allow(dead_code, reason = "each test file uses its own share")]

use peektrace_core::Agent;
use peektrace_core::stats::accumulate::{CorpusSummary, SampleRef, StatsWindow};
use peektrace_core::stats::detectors::causes::CauseKind;
use peektrace_core::stats::detectors::corpus::CorpusPanels;
use peektrace_core::stats::detectors::failures::FailRow;
use peektrace_core::stats::detectors::retries::CauseRow;
use peektrace_core::stats::detectors::rules::{RuleCheck, RuleMatch, Split};
use peektrace_core::stats::detectors::timing::TimeRow;
use peektrace_core::stats::detectors::waiting::WaitPanel;
use peektrace_core::stats::schema::{CauseId, Job, Kind};
use peektrace_core::stats::service::StatsReport;

pub const fn split(main: f64, nested: f64) -> Split {
    Split { main, nested }
}

/// `failRow`: one call in one session, `Bash exit 0`.
pub fn fail_row(key: &str, edit: impl FnOnce(&mut FailRow)) -> FailRow {
    let mut r = FailRow {
        key: key.to_owned(),
        label: key.to_owned(),
        r#where: "Bash exit 0".into(),
        piped: 0.0,
        rows: 1.0,
        sessions: 1.0,
        projects: 1.0,
        flagged: 0.0,
        note: None,
        detector: None,
        samples: Vec::new(),
    };
    edit(&mut r);
    r
}

/// `timeRow`: ten runs, 100 s, a 10% share.
pub fn time_row(key: Job, edit: impl FnOnce(&mut TimeRow)) -> TimeRow {
    let mut r = TimeRow {
        key,
        label: key.as_str(),
        runs: 10.0,
        co_present_runs: 0.0,
        total_sec: 100.0,
        share: 0.1,
        median_sec: 1.0,
        p95_sec: 2.0,
        capped_runs: 0.0,
        capped_sec: 0.0,
        bg_runs: 0.0,
        bg_sec: 0.0,
        samples: Vec::new(),
    };
    edit(&mut r);
    r
}

/// `causeRow`: the zsh glob-in-a-flag cause as the real corpus measured it.
pub fn cause_row(edit: impl FnOnce(&mut CauseRow)) -> CauseRow {
    let mut r = CauseRow {
        key: "cause:zsh-glob-flag".into(),
        cause: CauseId::ZshGlobFlag,
        kind: CauseKind::Rule,
        label: "unquoted glob in a flag value".into(),
        fix: Some("quote every glob passed as a flag value".into()),
        failing: true,
        hits: 1488.0,
        live_hits: None,
        flagged: 34.0,
        hits_by: split(125.0, 1363.0),
        pairs: 675.0,
        pairs_by: split(41.0, 634.0),
        retry_sessions: 550.0,
        sessions: 914.0,
        projects: 4.0,
        retry_sec: 2660.0,
        sec_measured: 675.0,
        retry_ctx_tokens: 32_313_906.0,
        tokens_measured: 675.0,
        per1k: split(25.8, 53.1),
        details: vec![("*.ts".into(), 1191.0)],
        diffs: vec![("--include=*.ts".into(), "--include='*.ts'".into(), 135.0)],
        rule: None,
        samples: Vec::new(),
        pair_samples: Vec::new(),
    };
    edit(&mut r);
    r
}

/// `FIRING`: a rule in `~/.claude/CLAUDE.md` that still fires.
pub fn firing() -> RuleCheck {
    RuleCheck {
        matches: vec![RuleMatch {
            path: "~/.claude/CLAUDE.md".into(),
            line: 100.0,
            excerpt: "Quote globs".into(),
        }],
        since_iso: Some("2026-10-15T19:06:47.272Z".into()),
        hits_through: Split::default(),
        hits_after: Split::default(),
        hits_since: split(42.0, 442.0),
        calls_since: split(1348.0, 7765.0),
        per1k_since: split(31.16, 56.92),
        advice: "setopt nonomatch in ~/.zshenv".into(),
    }
}

/// `report({causes, fails, time})`: a 30-day window, an empty corpus.
pub fn report(causes: Vec<CauseRow>, fails: Vec<FailRow>, time: Vec<TimeRow>) -> StatsReport {
    StatsReport {
        generated_at: 0.0,
        window: StatsWindow {
            days: 30.0,
            from_iso: "2026-09-21T00:00:00.000Z".into(),
            to_iso: "2026-10-21T00:00:00.000Z".into(),
        },
        corpus: CorpusSummary::default(),
        causes,
        fails,
        time,
        waiting: WaitPanel {
            calls: 0.0,
            total_sec: 0.0,
            share: 0.0,
            sessions: 0.0,
            projects: 0.0,
            median_sec: 0.0,
            p95_sec: 0.0,
            by_shape: Vec::new(),
            bg_runs: 0.0,
            bg_sec: 0.0,
            bg_declared_sec: 0.0,
            samples: Vec::new(),
        },
        clusters: Vec::new(),
        panels: CorpusPanels::default(),
        skipped: Vec::new(),
        caveats: Vec::new(),
        paths: Vec::new(),
    }
}

/// The three causes of `stats-format.test.tsx:259`.
pub fn three_causes() -> StatsReport {
    report(
        vec![
            cause_row(|_| {}),
            cause_row(|c| {
                c.key = "cause:zsh-equals-abort".into();
                c.cause = CauseId::ZshEqualsAbort;
                c.rule = Some(firing());
                c.fix = Some("quote ===".into());
            }),
            cause_row(|c| {
                c.key = "cause:command-not-found".into();
                c.cause = CauseId::CommandNotFound;
                c.kind = CauseKind::Env;
                c.fix = Some("install jq".into());
            }),
        ],
        vec![],
        vec![],
    )
}

pub const PARENT: &str = "aaaaaaaa-1111";

/// `session-chips.test.tsx` `sample`: a subagent call under [`PARENT`].
pub fn sample(edit: impl FnOnce(&mut SampleRef)) -> SampleRef {
    let mut s = SampleRef {
        sid: "sub-0".into(),
        agent: Agent::Claude,
        project: "demo-app".into(),
        kind: Kind::Subagent,
        parent: Some(PARENT.into()),
        seq: 0.0,
        tool_use_id: None,
        ts: None,
        line: "Bash: ls".into(),
    };
    edit(&mut s);
    s
}

/// A sample under another parent, at `seq`.
pub fn sample_at(sid: &str, parent: Option<&str>, seq: f64) -> SampleRef {
    sample(|s| {
        s.sid = sid.into();
        s.seq = seq;
        if let Some(p) = parent {
            s.parent = Some(p.into());
        }
    })
}

/// A main-session sample: no parent.
pub fn main_sample(sid: &str) -> SampleRef {
    sample(|s| {
        s.sid = sid.into();
        s.kind = Kind::Main;
        s.parent = None;
    })
}
