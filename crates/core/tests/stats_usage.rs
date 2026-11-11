//! The usage sink: day, month and block buckets over synthetic usage rows.
#![expect(clippy::float_cmp, reason = "turn counts are whole numbers")]

use std::cell::Cell;

use jiff::tz::TimeZone;
use peektrace_core::Agent;
use peektrace_core::js::time::{Utc, parse_date};
use peektrace_core::stats::detectors::usage::{
    DayKey, UsageBy, UsageResult, UsageSink, dedup_key_of,
};
use peektrace_core::stats::schema::{Fact, UsageFact};

const NOW: &str = "2026-08-10T12:00:00.000Z";

fn ms(iso: &str) -> i64 {
    parse_date(iso, &Utc).unwrap_or_default()
}

thread_local!(static N: Cell<u32> = const { Cell::new(0) });

fn usage(ts: &str, sid: &str, req: &str, agent: Agent) -> Fact {
    Fact::Usage(UsageFact {
        sid: sid.into(),
        agent,
        ts: Some(ts.into()),
        req: req.into(),
        model: "claude-sonnet-4-5".into(),
        input_tokens: 100.0,
        output_tokens: 50.0,
        cache_read_tokens: 0.0,
        cache_creation_tokens: 0.0,
    })
}

/// A Claude row with a fresh request id, so dedup stays out of the way.
fn at(ts: &str) -> Fact {
    let n = N.with(|n| {
        n.set(n.get() + 1);
        n.get()
    });
    usage(ts, "s1", &format!("req_{n}"), Agent::Claude)
}

fn zone(name: &str) -> TimeZone {
    TimeZone::get(name).unwrap_or(TimeZone::UTC)
}

fn run(
    facts: &[Fact],
    by: UsageBy,
    days: i64,
    now: &str,
    tz: &str,
    from: Option<i64>,
) -> UsageResult {
    let mut sink = UsageSink::new(by, days, from, ms(now), zone(tz), &Utc);
    for f in facts {
        sink.add(f);
    }
    sink.finish()
}

fn keys(r: &UsageResult) -> Vec<String> {
    r.rows.iter().map(|x| x.key.clone()).collect()
}

#[test]
fn block_mode_counts_claude_only() {
    let out = run(
        &[
            at("2026-08-10T09:30:00.000Z"),
            usage("2026-08-10T09:40:00.000Z", "c1", "x", Agent::Codex),
        ],
        UsageBy::Block,
        7,
        NOW,
        "UTC",
        None,
    );
    assert_eq!(out.rows.len(), 1);
    assert_eq!(out.rows[0].totals.agg.turns, 1.0);
    let b = out.rows[0].block.clone().unwrap_or_else(|| unreachable!());
    assert_eq!(b.start_iso, "2026-08-10T09:00:00.000Z");
    assert_eq!(b.end_iso, "2026-08-10T14:00:00.000Z");
    assert_eq!(b.last_iso, "2026-08-10T09:30:00.000Z");
    assert!(b.active);
    assert_eq!(out.skipped.other_agents, 1.0);
    assert!(out.caveats.iter().any(|c| c.contains("Claude turns only")));
}

#[test]
fn blocks_before_the_window_drop_and_overlaps_stay_whole() {
    let out = run(
        &[
            at("2026-08-02T23:00:00.000Z"),
            at("2026-08-03T13:00:00.000Z"),
        ],
        UsageBy::Block,
        7,
        NOW,
        "UTC",
        None,
    );
    assert_eq!(keys(&out), ["2026-08-03T13:00:00.000Z"]);
    let out = run(
        &[
            at("2026-08-09T10:30:00.000Z"),
            at("2026-08-09T13:00:00.000Z"),
        ],
        UsageBy::Block,
        1,
        NOW,
        "UTC",
        None,
    );
    assert_eq!(keys(&out), ["2026-08-09T10:00:00.000Z"]);
    assert_eq!(out.totals.agg.turns, 2.0);
}

#[test]
fn day_keys_follow_the_zone() {
    let facts = [
        at("2026-08-02T03:30:00.000Z"),
        at("2026-08-02T04:30:00.000Z"),
    ];
    assert_eq!(
        keys(&run(
            &facts,
            UsageBy::Day,
            30,
            NOW,
            "America/New_York",
            None
        )),
        ["2026-08-01", "2026-08-02"]
    );
    assert_eq!(
        keys(&run(&facts, UsageBy::Day, 30, NOW, "UTC", None)),
        ["2026-08-02"]
    );
    let mut key = DayKey::new(zone("Asia/Kolkata"));
    assert_eq!(key.of(ms("2026-08-01T18:29:59.000Z")), "2026-08-01");
    assert_eq!(key.of(ms("2026-08-01T18:30:00.000Z")), "2026-08-02");
    assert_eq!(key.of(ms("2026-08-01T18:20:00.000Z")), "2026-08-01");
}

#[test]
fn window_trim() {
    let out = run(
        &[
            at("2026-08-08T23:50:00.000Z"),
            at("2026-08-09T00:10:00.000Z"),
        ],
        UsageBy::Day,
        2,
        NOW,
        "UTC",
        None,
    );
    assert_eq!(keys(&out), ["2026-08-09"]);
    let out = run(
        &[
            at("2026-03-08T12:00:00.000Z"),
            at("2026-03-08T04:30:00.000Z"),
            at("2026-03-15T03:00:00.000Z"),
        ],
        UsageBy::Day,
        8,
        "2026-03-15T04:30:00.000Z",
        "America/New_York",
        None,
    );
    assert_eq!(keys(&out), ["2026-03-08", "2026-03-14"]);
    let out = run(
        &[
            at("2026-06-30T12:00:00.000Z"),
            at("2026-07-01T00:30:00.000Z"),
            at("2026-08-05T00:00:00.000Z"),
        ],
        UsageBy::Month,
        30,
        NOW,
        "UTC",
        None,
    );
    assert_eq!(keys(&out), ["2026-07", "2026-08"]);
    assert_eq!(out.totals.agg.turns, 2.0);
}

#[test]
fn dedup() {
    let ts = "2026-08-05T10:00:00.000Z";
    let out = run(
        &[
            usage(ts, "a", "req_same", Agent::Claude),
            usage(ts, "b", "req_same", Agent::Claude),
        ],
        UsageBy::Day,
        30,
        NOW,
        "UTC",
        None,
    );
    assert_eq!(out.totals.agg.turns, 1.0);
    assert_eq!(out.skipped.duplicates, 1.0);
    assert!(out.caveats.iter().any(|c| c.contains("count once")));
    let out = run(
        &[
            usage(ts, "a", "line-3", Agent::Claude),
            usage(ts, "b", "line-3", Agent::Claude),
            usage(ts, "p1", "msg-1", Agent::Pi),
            usage(ts, "p2", "msg-1", Agent::Pi),
            usage(ts, "p2", "msg-1", Agent::Pi),
        ],
        UsageBy::Day,
        30,
        NOW,
        "UTC",
        None,
    );
    assert_eq!(out.totals.agg.turns, 4.0);
    assert_eq!(dedup_key_of(Agent::Claude, "a", "req_x"), "req_x");
}
