//! The investigation prompts. Golden texts come from the TS `statsPrompt`
//! over the `prompts.test.ts` fixtures with the CLI number formatters
//! (`scripts/rust/stats-prompt-golden.ts`); each TS assertion is kept too.

use peektrace_core::Agent;
use peektrace_core::stats::accumulate::{CorpusSummary, SampleRef, StatsWindow};
use peektrace_core::stats::detectors::cause_row::PairSample;
use peektrace_core::stats::detectors::causes::CauseKind;
use peektrace_core::stats::detectors::corpus::CorpusPanels;
use peektrace_core::stats::detectors::failures::FailRow;
use peektrace_core::stats::detectors::retries::CauseRow;
use peektrace_core::stats::detectors::rules::{RuleCheck, RuleMatch, Split};
use peektrace_core::stats::detectors::timing::TimeRow;
use peektrace_core::stats::detectors::waiting::{WaitPanel, WaitShapeRow};
use peektrace_core::stats::prompts::{PromptTarget, stats_prompt};
use peektrace_core::stats::schema::{CauseId, Job, Kind, WaitShape};
use peektrace_core::stats::service::StatsReport;

const GOLDEN: &str = include_str!("fixtures/stats-prompts.json");
const SUB: &str = "agent-a8cc4430796a7efce";
const PARENT: &str = "92ea6c7d-b251-4d46-af59-c5772244e477";

fn path() -> String {
    format!("/home/me/.claude/projects/-home-me-work/{PARENT}/subagents/{SUB}.jsonl")
}

fn golden(name: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(GOLDEN).expect("golden json");
    v[name].as_str().expect("golden case").to_owned()
}

fn split(main: f64, nested: f64) -> Split {
    Split { main, nested }
}

fn sample(edit: impl FnOnce(&mut SampleRef)) -> SampleRef {
    let mut s = SampleRef {
        sid: SUB.into(),
        agent: Agent::Claude,
        project: "-home-me-work".into(),
        kind: Kind::Subagent,
        parent: Some(PARENT.into()),
        seq: 18.0,
        tool_use_id: Some("toolu_01".into()),
        ts: Some("2026-10-15T07:46:55.123Z".into()),
        line: "grep -rn \"x\" --include=*.ts .".into(),
    };
    edit(&mut s);
    s
}

fn firing() -> RuleCheck {
    RuleCheck {
        matches: vec![RuleMatch {
            path: "~/.claude/CLAUDE.md".into(),
            line: 101.0,
            excerpt: "Quote globs".into(),
        }],
        since_iso: Some("2026-10-01T00:00:00.000Z".into()),
        hits_since: split(3.0, 40.0),
        calls_since: split(1000.0, 2000.0),
        per1k_since: split(3.0, 20.0),
        advice: "if it keeps firing: setopt nonomatch in ~/.zshenv".into(),
    }
}

fn unmeasured() -> RuleCheck {
    RuleCheck {
        calls_since: split(0.0, 0.0),
        hits_since: split(0.0, 0.0),
        ..firing()
    }
}

fn cause_row(edit: impl FnOnce(&mut CauseRow)) -> CauseRow {
    let mut r = CauseRow {
        key: "cause:zsh-glob-flag".into(),
        cause: CauseId::ZshGlobFlag,
        kind: CauseKind::Rule,
        label: "unquoted glob in a flag value".into(),
        fix: Some("quote the glob".into()),
        failing: true,
        hits: 1492.0,
        flagged: 35.0,
        hits_by: split(125.0, 1367.0),
        pairs: 678.0,
        pairs_by: split(41.0, 637.0),
        retry_sessions: 552.0,
        sessions: 913.0,
        projects: 4.0,
        retry_sec: 2698.0,
        sec_measured: 678.0,
        retry_ctx_tokens: 32_000.0,
        tokens_measured: 678.0,
        per1k: split(25.0, 44.0),
        details: vec![("*.ts".into(), 1196.0)],
        diffs: vec![("--include=*.ts".into(), "--include='*.ts'".into(), 145.0)],
        rule: None,
        samples: Vec::new(),
        pair_samples: Vec::new(),
    };
    edit(&mut r);
    r
}

fn fail_row(edit: impl FnOnce(&mut FailRow)) -> FailRow {
    let mut r = FailRow {
        key: "scan:tsc".into(),
        label: "error TS####".into(),
        r#where: "Bash exit 0".into(),
        rows: 206.0,
        sessions: 137.0,
        projects: 2.0,
        flagged: 0.0,
        note: Some("186 of 206 pipe into head/tail/grep".into()),
        detector: Some("piped-exit-code".into()),
        samples: Vec::new(),
    };
    edit(&mut r);
    r
}

fn time_row(edit: impl FnOnce(&mut TimeRow)) -> TimeRow {
    let mut r = TimeRow {
        key: Job::WaitPoll,
        label: "waiting",
        runs: 643.0,
        co_present_runs: 0.0,
        total_sec: 13_196.0,
        share: 0.5,
        median_sec: 4.0,
        p95_sec: 87.0,
        capped_runs: 0.0,
        capped_sec: 0.0,
        bg_runs: 0.0,
        bg_sec: 0.0,
        samples: Vec::new(),
    };
    edit(&mut r);
    r
}

fn waiting(calls: f64) -> WaitPanel {
    WaitPanel {
        calls,
        total_sec: 13_196.0,
        share: 0.5,
        sessions: 197.0,
        projects: 35.0,
        median_sec: 4.0,
        p95_sec: 87.0,
        by_shape: vec![
            WaitShapeRow {
                key: WaitShape::LoopPoll,
                calls: 207.0,
                total_sec: 8956.0,
                share: 0.1,
            },
            WaitShapeRow {
                key: WaitShape::Watcher,
                calls: 0.0,
                total_sec: 0.0,
                share: 0.0,
            },
        ],
        bg_runs: 307.0,
        bg_sec: 2688.0,
        bg_declared_sec: 14_608.0,
        samples: Vec::new(),
    }
}

fn report(causes: Vec<CauseRow>, fails: Vec<FailRow>, time: Vec<TimeRow>) -> StatsReport {
    StatsReport {
        generated_at: 0.0,
        window: StatsWindow {
            days: 30.0,
            from_iso: "2026-09-21T17:48:33.960Z".into(),
            to_iso: "2026-10-21T17:48:33.960Z".into(),
        },
        corpus: CorpusSummary {
            transcripts: 1821.0,
            sessions: 1821.0,
            projects: 132.0,
            tool_calls: 38_749.0,
            bash_calls: 35_625.0,
            bash_sec: 74_825.0,
            tool_sec: 81_062.0,
            human_wait_sec: 0.0,
            kinds: vec![
                ("main".into(), 401.0),
                ("subagent".into(), 163.0),
                ("workflow-agent".into(), 1257.0),
            ],
            agents: vec![("claude".into(), 1821.0)],
        },
        causes,
        fails,
        time,
        waiting: waiting(643.0),
        clusters: Vec::new(),
        panels: CorpusPanels::default(),
        skipped: Vec::new(),
        caveats: Vec::new(),
        paths: vec![(SUB.into(), path())],
    }
}

fn full() -> StatsReport {
    report(
        vec![
            cause_row(|r| r.rule = Some(firing())),
            cause_row(|r| {
                r.key = "cause:output-too-large".into();
                r.cause = CauseId::OutputTooLarge;
                r.failing = false;
            }),
            cause_row(|r| {
                r.key = "cause:sleep-blocked".into();
                r.cause = CauseId::SleepBlocked;
                r.hits = 0.0;
            }),
        ],
        vec![
            fail_row(|_| {}),
            fail_row(|r| {
                r.key = "flag:Bash".into();
                r.label = "Bash errors".into();
                r.note = None;
            }),
        ],
        vec![
            time_row(|_| {}),
            time_row(|r| {
                r.key = Job::Git;
                r.label = "git";
                r.total_sec = 9060.0;
                r.share = 0.12;
            }),
        ],
    )
}

fn prompt(r: &StatsReport, t: &PromptTarget) -> String {
    stats_prompt(r, None, t).unwrap_or_default()
}

fn cause(key: &str) -> PromptTarget {
    PromptTarget::Cause(key.to_owned())
}

#[test]
fn report_prompt_leads_with_the_goal_and_fits_one_screen() {
    let text = prompt(&full(), &PromptTarget::Report);
    assert_eq!(text, golden("report"));
    assert!(text.starts_with("Goal: make my coding agents faster and cheaper"));
    assert!(text.contains("don't apply until I confirm"));
    assert!(text.split('\n').count() <= 30);
}

#[test]
fn report_prompt_states_window_corpus_and_top_rows() {
    let text = prompt(&full(), &PromptTarget::Report);
    for want in [
        "(last 30 days, all agents)",
        "Data (2026-09-21 → 2026-10-21):",
        "1,821 transcripts (401 main, 1,420 subagent), 132 projects",
        "avoidable retries: 1,356 round trips, 1h29m, 64.0k ctx tokens re-read",
        "  - zsh-glob-flag: 1,492 hits · 678 retries (main 41 / sub 637) · 44m58s · 32.0k ctx tokens re-read · rule in memory\n",
        "  - output-too-large: 1,492 hits · 678 read-backs",
        "  - scan:tsc (error TS####): 206 calls, 137 sessions · 186 of 206",
        "  - flag:Bash (Bash errors): 206 calls, 137 sessions\n",
        "shell time (--time job): wait/poll 3h39m (50.0%) · git 2h31m (12.0%)",
        "waiting: 643 calls, 3h39m, 307 backgrounded",
    ] {
        assert!(text.contains(want), "{want}");
    }
    assert!(!text.contains("sleep-blocked"));
}

#[test]
fn report_prompt_names_a_rule_that_still_fires() {
    let text = prompt(&full(), &PromptTarget::Report);
    assert!(text.contains(
        "rules in memory but still firing: zsh-glob-flag (rule in ~/.claude/CLAUDE.md:101 · still firing"
    ));
    let quiet = RuleCheck {
        hits_since: split(0.0, 0.0),
        per1k_since: split(0.0, 0.0),
        ..firing()
    };
    let q = prompt(
        &report(vec![cause_row(|r| r.rule = Some(quiet))], vec![], vec![]),
        &PromptTarget::Report,
    );
    assert_eq!(q, golden("quiet"));
    assert!(q.contains("rules in memory but still firing: none"));
    let none = prompt(
        &report(vec![cause_row(|_| {})], vec![], vec![]),
        &PromptTarget::Report,
    );
    assert_eq!(none, golden("norule"));
    assert!(!none.contains("rules in memory"));
}

#[test]
fn an_unmeasured_rule_is_never_reported_as_working() {
    let text = prompt(
        &report(
            vec![cause_row(|r| r.rule = Some(unmeasured()))],
            vec![],
            vec![],
        ),
        &PromptTarget::Report,
    );
    assert_eq!(text, golden("unmeasured"));
    assert!(text.contains("- rules in memory, still-firing rate not measured yet: zsh-glob-flag"));
    assert!(!text.contains("still firing: none"));
}

#[test]
fn an_empty_window_prints_no_empty_section() {
    let mut r = report(vec![], vec![], vec![]);
    r.waiting = waiting(0.0);
    let text = prompt(&r, &PromptTarget::Report);
    assert_eq!(text, golden("empty"));
    for gone in [
        "- causes",
        "- failures",
        "- shell time",
        "avoidable retries",
    ] {
        assert!(!text.contains(gone), "{gone}");
    }
    assert!(!text.split('\n').any(|l| l.ends_with(": ")));
}

#[test]
fn report_prompt_ends_with_real_commands_in_its_scope() {
    let text = prompt(&full(), &PromptTarget::Report);
    assert!(text.contains("- full report: peektrace stats --days 30 --json"));
    assert!(text.contains(
        "- one row, with transcript paths: peektrace stats prompt --days 30 --cause <id>"
    ));
    assert!(text.contains("- a row's calls: peektrace stats drill --days 30 --cause <id> --json"));
    let scoped =
        stats_prompt(&full(), Some(Agent::Codex), &PromptTarget::Report).unwrap_or_default();
    assert_eq!(scoped, golden("scoped"));
    assert!(scoped.contains("(last 30 days, codex)"));
    assert!(scoped.contains("peektrace stats --days 30 --agent codex --json"));
}

fn cause_report() -> StatsReport {
    let pair = PairSample {
        failed: sample(|_| {}),
        retry: sample(|s| {
            s.seq = 19.0;
            s.tool_use_id = Some("toolu_02".into());
            s.line = "grep -rn \"x\" --include='*.ts' .".into();
        }),
    };
    let row = cause_row(|r| {
        r.rule = Some(firing());
        r.pair_samples = vec![pair];
        r.samples = vec![
            sample(|_| {}),
            sample(|s| {
                s.sid = "lost".into();
                s.parent = None;
                s.seq = 3.0;
                s.tool_use_id = None;
                s.ts = None;
                s.line = "ls *.md".into();
            }),
        ];
    });
    report(vec![row], vec![], vec![])
}

#[test]
fn a_cause_prompt_is_goal_data_samples_and_drill() {
    let r = cause_report();
    let text = prompt(&r, &cause("zsh-glob-flag"));
    assert_eq!(text, golden("cause"));
    let p = path();
    assert!(text.contains(&format!("- 2026-10-15T07:46:55Z · toolu_01 · {p}")));
    assert!(text.ends_with(
        "Calls (first 200, .total counts all, .paths has transcripts): peektrace stats drill --days 30 --cause zsh-glob-flag --json"
    ));
}

#[test]
fn the_full_cause_key_finds_the_same_row() {
    let r = cause_report();
    assert_eq!(
        prompt(&r, &cause("cause:zsh-glob-flag")),
        prompt(&r, &cause("zsh-glob-flag"))
    );
}

#[test]
fn rule_cause_without_a_file_says_so_env_cause_does_not() {
    let none = prompt(
        &report(vec![cause_row(|_| {})], vec![], vec![]),
        &cause("zsh-glob-flag"),
    );
    assert_eq!(none, golden("none"));
    assert!(none.contains("- rule: none in AGENTS.md/CLAUDE.md"));
    assert!(
        none.contains("- cause: zsh-glob-flag · fix lives in: memory file (AGENTS.md/CLAUDE.md)")
    );
    let env = prompt(
        &report(
            vec![cause_row(|r| {
                r.kind = CauseKind::Env;
                r.key = "cause:command-not-found".into();
                r.cause = CauseId::CommandNotFound;
            })],
            vec![],
            vec![],
        ),
        &cause("command-not-found"),
    );
    assert_eq!(env, golden("env"));
    assert!(env.contains("fix lives in: host environment"));
    assert!(!env.contains("- rule"));
}

#[test]
fn a_rule_too_new_to_measure_passes_on_the_advice() {
    let fresh = prompt(
        &report(
            vec![cause_row(|r| r.rule = Some(unmeasured()))],
            vec![],
            vec![],
        ),
        &cause("zsh-glob-flag"),
    );
    assert_eq!(fresh, golden("fresh"));
    assert!(fresh.contains(
        "not measured yet (added 2026-10-01)\n  If it keeps firing: setopt nonomatch in ~/.zshenv."
    ));
    assert!(!fresh.contains("still fires"));
}

#[test]
fn an_unknown_key_is_no_prompt() {
    let r = cause_report();
    assert_eq!(stats_prompt(&r, None, &cause("nope")), None);
    assert_eq!(
        stats_prompt(&r, None, &PromptTarget::Fail("nope".into())),
        None
    );
    assert_eq!(
        stats_prompt(&r, None, &PromptTarget::Time("test".into())),
        None
    );
}

#[test]
fn a_fail_prompt_names_key_detector_note_and_calls() {
    let r = report(
        vec![],
        vec![fail_row(|r| {
            r.samples = vec![sample(|s| s.line = "bun run typecheck | tail".into())];
        })],
        vec![],
    );
    let text = prompt(&r, &PromptTarget::Fail("scan:tsc".into()));
    assert_eq!(text, golden("fail"));
    for want in [
        "Pattern: error TS#### (Bash exit 0)",
        "- key: scan:tsc · detector: piped-exit-code",
        "- calls: 206 (0 set the error flag) in 137 sessions, 2 projects",
        "- note: 186 of 206 pipe into head/tail/grep",
    ] {
        assert!(text.contains(want), "{want}");
    }
    let p = path();
    assert!(text.contains(&format!(
        "Samples:\n- 2026-10-15T07:46:55Z · toolu_01 · {p}\n  call: bun run typecheck | tail"
    )));
    assert!(text.ends_with("peektrace stats drill --days 30 --fail scan:tsc --json"));
}

#[test]
fn the_waiting_row_carries_the_waiting_shapes() {
    let r = report(
        vec![],
        vec![],
        vec![time_row(|r| {
            r.capped_runs = 8.0;
            r.capped_sec = 2880.0;
        })],
    );
    let text = prompt(&r, &PromptTarget::Time("wait/poll".into()));
    assert_eq!(text, golden("time"));
    assert!(text.contains("- note: 8 of 643 runs returned at a harness ceiling"));
    assert!(text.contains("peektrace stats drill --days 30 --time wait/poll --json"));
}
