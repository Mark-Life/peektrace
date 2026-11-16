//! The time table and the waiting panel: exclusive attribution, harness
//! ceilings, and one function behind the wait row and the panel.
#![allow(clippy::float_cmp, reason = "values are rounded to fixed digits")]

mod common;

use common::bash_fact;
use peektrace_core::stats::accumulate::{
    CorpusTotals, DrillHit, DrillTable, DrillTarget, FinishCtx, StatsWindow,
};
use peektrace_core::stats::detectors::timing::{
    TimeRow, TimeTable, classify_job, is_backgrounded, is_capped, segments,
};
use peektrace_core::stats::detectors::waiting::{
    WaitPanel, WaitPanelSink, declared_delay_sec, is_waiting, wait_shape,
};
use peektrace_core::stats::redact::Redactor;
use peektrace_core::stats::schema::{CallFact, Fact, Job, WaitShape};

/// One bash fact, minted the way extraction mints it.
fn fact_of(cmd: &str, dur_sec: f64, sid: &str, seq: f64) -> CallFact {
    let mut f = bash_fact(cmd);
    let (job, present) = classify_job(cmd);
    sid.clone_into(&mut f.base.sid);
    "p1".clone_into(&mut f.base.project);
    f.seq = seq;
    f.dur_sec = Some(dur_sec);
    let b = f.bash.as_deref_mut().expect("bash");
    cmd.split_whitespace()
        .next()
        .unwrap_or("")
        .clone_into(&mut b.bin);
    cmd.clone_into(&mut b.family);
    b.job = job;
    b.present = present;
    b.wait = wait_shape(cmd);
    b.declared_delay_sec = declared_delay_sec(cmd);
    b.piped = cmd.contains('|');
    b.bg = is_backgrounded(cmd);
    b.capped = is_capped(Some(dur_sec));
    f
}

fn count(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

struct Run {
    caveats: Vec<String>,
    facts: Vec<CallFact>,
    panel: WaitPanel,
    rows: Vec<TimeRow>,
    time_hits: Vec<DrillHit>,
    wait_hits: Vec<DrillHit>,
}

fn run(lines: &[(&str, f64, &str)], drill: Option<&DrillTarget>) -> Run {
    let facts: Vec<CallFact> = lines
        .iter()
        .zip(0u32..)
        .map(|(&(cmd, dur, sid), seq)| fact_of(cmd, dur, sid, f64::from(seq)))
        .collect();
    let mut totals = CorpusTotals::default();
    let mut time = TimeTable::new(drill);
    let mut waiting = WaitPanelSink::new(drill);
    for f in &facts {
        let fact = Fact::Call(f.clone());
        totals.add(&fact);
        time.add(&fact);
        waiting.add(&fact);
    }
    let corpus = totals.finish();
    let window = StatsWindow {
        days: 60.0,
        from_iso: String::new(),
        to_iso: String::new(),
    };
    let redact = Redactor::new(true);
    let mut ctx = FinishCtx {
        corpus: &corpus,
        window: &window,
        redact: &redact,
        limit: 20.0,
        sample_cap: 3,
        caveats: Vec::new(),
    };
    let panel = waiting.finish(&ctx);
    let rows = time.finish(&mut ctx);
    let time_hits = time.drill(&ctx);
    let wait_hits = waiting.drill(&ctx);
    Run {
        caveats: ctx.caveats,
        facts,
        panel,
        rows,
        time_hits,
        wait_hits,
    }
}

const CORPUS: [(&str, f64, &str); 10] = [
    ("bun run typecheck && bun test", 30.0, "s1"),
    ("bun test packages/core", 12.0, "s1"),
    ("tsc --noEmit", 8.0, "s1"),
    ("git status && ls -la", 0.4, "s1"),
    ("grep -rn \"foo\" src | head -20", 0.2, "s1"),
    ("sleep 30; curl localhost:3000/health", 31.0, "s2"),
    (
        "until [ \"$(curl -s -o /dev/null -w %{http_code} localhost:5173)\" = 200 ]; do sleep 5; done",
        600.0,
        "s2",
    ),
    ("python3 scripts/probe.py", 3.0, "s1"),
    ("cat package.json", 0.05, "s1"),
    ("sleep 1980 && echo APOLLO_WINDOW_OPEN &", 0.17, "s3"),
];

fn row(rows: &[TimeRow], key: Job) -> &TimeRow {
    rows.iter().find(|r| r.key == key).expect("row")
}

#[test]
fn compound_line_lands_in_one_job() {
    let (job, mut present) = classify_job("bun run typecheck && bun test");
    assert_eq!(job, Job::Test);
    present.sort_by_key(|j| j.as_str());
    assert_eq!(present, [Job::Test, Job::Typecheck]);
}

#[test]
fn waiting_outranks_everything() {
    let (job, present) =
        classify_job("while ! grep -q PASS /tmp/test.log; do sleep 5; bun test; done");
    assert_eq!(job, Job::WaitPoll);
    assert!(present.contains(&Job::Test));
}

#[test]
fn the_charged_job_is_always_present() {
    for cmd in ["env -u GITHUB_TOKEN gh pr list", "wibble --frobnicate"] {
        let (job, present) = classify_job(cmd);
        assert!(present.contains(&job), "{cmd}");
    }
}

#[test]
fn wrapper_cd_and_path_prefix_reduce_to_the_binary() {
    assert_eq!(
        segments("cd /tmp/x && ../../.venv/bin/python -c 'print(1)'"),
        ["python -c 'print(1)'"]
    );
    assert_eq!(
        segments("timeout 5m sudo docker build ."),
        ["docker build ."]
    );
}

#[test]
fn harness_ceilings() {
    for s in [600.0, 590.0, 120.0, 118.0, 124.0] {
        assert!(is_capped(Some(s)), "{s}");
    }
    for s in [100.0, 125.0] {
        assert!(!is_capped(Some(s)), "{s}");
    }
    assert!(!is_capped(None));
}

#[test]
fn a_capped_run_is_split_out_of_its_job_total() {
    let r = run(&CORPUS, None);
    let wait = row(&r.rows, Job::WaitPoll);
    assert_eq!((wait.capped_runs, wait.capped_sec), (1.0, 600.0));
    assert!(wait.total_sec > wait.capped_sec);
    assert!(r.caveats.iter().any(|c| c.contains("harness ceiling")));
}

#[test]
fn every_line_is_charged_once() {
    let r = run(&CORPUS, None);
    let runs: f64 = r.rows.iter().map(|x| x.runs).sum();
    let shares: f64 = r.rows.iter().map(|x| x.share).sum();
    let seconds: f64 = r.rows.iter().map(|x| x.total_sec).sum();
    let want: f64 = r.facts.iter().map(|f| f.dur_sec.unwrap_or(0.0)).sum();
    assert_eq!(runs, count(r.facts.len()));
    assert!((shares - 1.0).abs() < 5e-4);
    assert!((seconds - want).abs() < 5e-3);
}

#[test]
fn co_present_counts_sit_beside_attributed_ones() {
    let r = run(&CORPUS, None);
    let t = row(&r.rows, Job::Typecheck);
    assert_eq!((t.runs, t.co_present_runs), (1.0, 2.0));
    for x in &r.rows {
        assert!(x.co_present_runs >= x.runs);
    }
}

#[test]
fn rows_rank_by_total_seconds() {
    let r = run(&CORPUS, None);
    let totals: Vec<f64> = r.rows.iter().map(|x| x.total_sec).collect();
    let mut sorted = totals.clone();
    sorted.sort_by(|a, b| b.total_cmp(a));
    assert_eq!(sorted, totals);
}

#[test]
fn wait_shapes() {
    for (cmd, shape) in [
        (
            "until [ -f /tmp/ready ]; do sleep 5; done",
            WaitShape::LoopPoll,
        ),
        (
            "for i in $(seq 1 60); do gh pr view 1; sleep 5; done",
            WaitShape::LoopPoll,
        ),
        (
            "python3 -c \"import time; time.sleep(560); print(1)\"",
            WaitShape::BareDelay,
        ),
        (
            "perl -e 'select undef,undef,undef,10'",
            WaitShape::BareDelay,
        ),
        ("/bin/sleep 30; echo ok", WaitShape::BareDelay),
        (
            "bun run main.ts > /tmp/x.log 2>&1 & sleep 5; tail -12 /tmp/x.log",
            WaitShape::BareDelay,
        ),
        (
            "gh run watch 123 --exit-status --interval 20",
            WaitShape::Watcher,
        ),
        ("curl --retry 5 https://example.com", WaitShape::CurlRetry),
    ] {
        assert_eq!(wait_shape(cmd), Some(shape), "{cmd}");
    }
}

#[test]
fn does_not_wait() {
    for cmd in [
        "f url1 & f url2 & wait",
        "pgrep -P 4481 | while read p; do ps -o command= -p $p; done",
        "for i in $(seq 1 12); do curl localhost; done",
        "curl \"https://api.github.com/x?since=2022-01-01&until=2023-06-01\"",
        ".venv/bin/python -c \"for d in xs: r = get(d); time.sleep(1.2)\"",
        "vitest --watch",
        "grep -iE \"vite|watch\" /tmp/log",
        "cat > run.sh <<'EOF'\nsleep 25\nEOF",
    ] {
        assert_eq!(wait_shape(cmd), None, "{cmd}");
    }
}

#[test]
fn comment_apostrophe_and_heredoc_bodies() {
    assert!(is_waiting("# anon wouldn't see this\nsleep 30; echo done"));
    assert_eq!(
        declared_delay_sec("cat > run.sh <<'EOF'\nsleep 25\nEOF"),
        0.0
    );
    assert_eq!(declared_delay_sec("sleep 30 && sleep 12"), 42.0);
}

#[test]
fn panel_and_wait_row_read_the_same_numbers() {
    let r = run(&CORPUS, None);
    let w = row(&r.rows, Job::WaitPoll);
    assert_eq!(r.panel.calls, w.runs);
    assert_eq!(r.panel.total_sec, w.total_sec);
    assert_eq!(r.panel.share, w.share);
}

#[test]
fn shapes_partition_the_panel() {
    let r = run(&CORPUS, None);
    let calls: f64 = r.panel.by_shape.iter().map(|s| s.calls).sum();
    assert_eq!(calls, r.panel.calls);
    let mut keys: Vec<&str> = r.panel.by_shape.iter().map(|s| s.key.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["bare-delay", "curl-retry", "loop-poll", "watcher"]);
}

#[test]
fn a_backgrounded_wait_never_sums_in() {
    let p = run(&CORPUS, None).panel;
    assert_eq!(
        (p.bg_runs, p.bg_sec, p.bg_declared_sec),
        (1.0, 0.17, 1980.0)
    );
    assert!(p.total_sec < p.bg_declared_sec);
}

fn target(table: DrillTable, key: &str) -> DrillTarget {
    DrillTarget {
        table,
        key: key.to_owned(),
    }
}

#[test]
fn drill() {
    let r = run(&CORPUS, None);
    assert!(r.time_hits.is_empty() && r.wait_hits.is_empty());
    let r = run(&CORPUS, Some(&target(DrillTable::Time, "wait/poll")));
    let runs = row(&r.rows, Job::WaitPoll).runs;
    assert_eq!(count(r.time_hits.len()), runs);
    let durs: Vec<Option<f64>> = r.time_hits.iter().map(|h| h.dur_sec).collect();
    assert_eq!(durs, [Some(600.0), Some(31.0), Some(0.17)]);
    assert_eq!(
        run(&CORPUS, Some(&target(DrillTable::Wait, "loop-poll")))
            .wait_hits
            .len(),
        1
    );
    assert_eq!(
        run(&CORPUS, Some(&target(DrillTable::Wait, "all")))
            .wait_hits
            .len(),
        3
    );
}
