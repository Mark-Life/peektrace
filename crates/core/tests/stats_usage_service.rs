//! `stats usage` end to end over a synthetic Claude projects dir: request
//! checks, the widened first pass, and the block chain across the cutoff.
#![allow(clippy::float_cmp, reason = "turn counts are whole numbers")]

mod common;

use std::cell::Cell;

use common::TempDir;
use peektrace_core::js::time::{Utc, to_iso_string};
use peektrace_core::roots::Source;
use peektrace_core::stats::cache::StatsCache;
use peektrace_core::stats::detectors::usage::UsageBy;
use peektrace_core::stats::service::{StatsEnv, StatsError};
use peektrace_core::stats::usage_service::{UsageReport, UsageRequest, usage};
use serde_json::json;

const NOW: i64 = 1_786_363_200_000; // 2026-08-10T12:00:00.000Z
const HOUR: i64 = 3_600_000;
const DAY: i64 = 24 * HOUR;

/// One transcript holding one assistant turn per `(ms, request id)`.
fn corpus(calls: &[(i64, &str)]) -> TempDir {
    let t = TempDir::new("usage");
    let lines: Vec<String> = calls
        .iter()
        .map(|(ms, req)| {
            json!({
                "type": "assistant",
                "requestId": req,
                "timestamp": to_iso_string(*ms),
                "cwd": "/w",
                "message": {
                    "model": "claude-sonnet-4-5",
                    "usage": { "input_tokens": 100, "output_tokens": 50 },
                    "content": [{ "type": "text", "text": "x" }],
                },
            })
            .to_string()
        })
        .collect();
    t.write("projects/-w/sess-1.jsonl", &(lines.join("\n") + "\n"));
    t
}

struct Run {
    passes: usize,
    report: Result<UsageReport, StatsError>,
}

fn run(t: &TempDir, req: &UsageRequest, now: i64) -> Run {
    let claude = [Source {
        id: "claude".into(),
        label: "default".into(),
        home: t.path(".claude"),
        projects_root: t.path("projects"),
    }];
    let passes = Cell::new(0);
    let progress = |done: usize, total: usize| {
        if done == total {
            passes.set(passes.get() + 1);
        }
    };
    #[allow(clippy::cast_precision_loss, reason = "epoch ms fit in f64")]
    let env = StatsEnv {
        claude: &claude,
        codex_home: t.path(".codex"),
        pi_home: t.path(".pi"),
        cache: StatsCache::new(t.path("pdir/stats-rs")),
        local: &Utc,
        now_ms: now as f64,
        progress: Some(&progress),
    };
    let report = usage(&env, req);
    Run {
        passes: passes.get(),
        report,
    }
}

fn req(by: UsageBy, days: Option<i64>, tz: &str) -> UsageRequest {
    UsageRequest {
        agents: None,
        by,
        days,
        force: false,
        tz: Some(tz.to_owned()),
    }
}

fn reason_of(r: &UsageRequest) -> String {
    let t = corpus(&[]);
    match run(&t, r, NOW).report {
        Err(e) => e.to_string(),
        Ok(_) => String::new(),
    }
}

#[test]
fn days_out_of_range_fail_with_a_days_message() {
    for days in [0, -3, 1_000_000_000] {
        let reason = reason_of(&req(UsageBy::Day, Some(days), "UTC"));
        assert!(
            reason.starts_with("days must be a whole number"),
            "{days}: {reason}"
        );
    }
}

#[test]
fn an_unknown_zone_names_the_zone() {
    assert_eq!(
        reason_of(&req(UsageBy::Day, None, "Mars/Olympus")),
        "unknown time zone: Mars/Olympus"
    );
}

#[test]
fn block_mode_widens_the_pass_by_one_day() {
    // a: 7d 3h back, inside the widened day; b: 4h later, past the cutoff.
    let cutoff = NOW - 7 * DAY;
    let t = corpus(&[(cutoff - 3 * HOUR, "a"), (cutoff + HOUR, "b")]);
    let r = run(&t, &req(UsageBy::Block, Some(7), "UTC"), NOW);
    assert_eq!(r.passes, 1);
    let out = r.report.expect("report");
    let rows: Vec<(String, f64)> = out
        .result
        .rows
        .iter()
        .map(|x| (x.key.clone(), x.totals.agg.turns))
        .collect();
    assert_eq!(rows, [(to_iso_string(cutoff - 3 * HOUR), 2.0)]);
}

fn blocks_for(calls: &[(i64, &str)], days: i64, now: i64) -> (usize, Vec<(String, f64)>) {
    let t = corpus(calls);
    let r = run(&t, &req(UsageBy::Block, Some(days), "UTC"), now);
    let out = r.report.expect("report");
    let rows = out
        .result
        .rows
        .iter()
        .map(|x| (x.key.clone(), x.totals.agg.turns))
        .collect();
    (r.passes, rows)
}

#[test]
fn a_call_far_before_the_cutoff_still_sets_the_chain() {
    let cutoff = NOW - DAY;
    let calls = [
        (cutoff - 11 * HOUR / 2, "a"),
        (cutoff - HOUR, "b"),
        (cutoff + 3 * HOUR, "c"),
    ];
    let late = to_iso_string(cutoff + 3 * HOUR);
    assert_eq!(blocks_for(&calls, 1, NOW).1, [(late.clone(), 1.0)]);
    assert_eq!(
        blocks_for(&calls, 2, NOW).1,
        [(to_iso_string(cutoff - 6 * HOUR), 2.0), (late, 1.0)]
    );
}

#[test]
fn a_block_that_ended_before_the_cutoff_does_not_come_back() {
    let now = 1_786_365_000_000; // 2026-08-10T12:30:00.000Z
    let calls = [
        (1_786_258_800_000, "a"), // 2026-08-09T07:00:00.000Z
        (1_786_262_400_000, "b"), // 2026-08-09T08:00:00.000Z
    ];
    assert_eq!(blocks_for(&calls, 1, now).1, []);
}

#[test]
fn steady_work_through_the_widened_range_reads_full_history() {
    let cutoff = NOW - DAY;
    let reqs: Vec<String> = (0..12).map(|i| format!("r{i}")).collect();
    let calls: Vec<(i64, &str)> = (0..12)
        .map(|i| {
            (
                cutoff - 40 * HOUR + i * 4 * HOUR,
                reqs[usize::try_from(i).unwrap_or(0)].as_str(),
            )
        })
        .collect();
    let (passes, rows) = blocks_for(&calls, 1, NOW);
    assert_eq!(passes, 2);
    assert_eq!(rows, [(to_iso_string(cutoff), 2.0)]);
}

#[test]
fn a_warm_run_leaves_the_index_alone_until_a_transcript_changes() {
    let t = corpus(&[(NOW - HOUR, "a")]);
    let r = req(UsageBy::Day, Some(7), "UTC");
    let index = t.path("pdir/stats-rs/index.json");
    let mtime = || {
        std::fs::metadata(&index)
            .and_then(|m| m.modified())
            .expect("index")
    };
    let age = |at: std::time::SystemTime| {
        std::fs::File::options()
            .write(true)
            .open(&index)
            .and_then(|f| f.set_modified(at))
            .expect("touch");
    };
    let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
    run(&t, &r, NOW).report.expect("cold");
    age(old);
    run(&t, &r, NOW).report.expect("warm");
    assert_eq!(mtime(), old);
    t.write(
        "projects/-w/sess-2.jsonl",
        &(json!({ "type": "user", "timestamp": to_iso_string(NOW - HOUR), "cwd": "/w" })
            .to_string()
            + "\n"),
    );
    run(&t, &r, NOW).report.expect("changed");
    assert_ne!(mtime(), old);
}
