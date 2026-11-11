//! The usage sink: day, month and block buckets over synthetic usage rows.

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

fn with(f: Fact, edit: impl FnOnce(&mut UsageFact)) -> Fact {
    let Fact::Usage(mut u) = f else {
        unreachable!("a usage fact")
    };
    edit(&mut u);
    Fact::Usage(u)
}

fn block_starts(out: &UsageResult) -> Vec<(String, f64, bool)> {
    out.rows
        .iter()
        .map(|r| {
            let b = r.block.clone().unwrap_or_else(|| unreachable!());
            (b.start_iso, r.totals.agg.turns, b.active)
        })
        .collect()
}

fn blocks(ts: &[&str], now: &str) -> UsageResult {
    let facts: Vec<Fact> = ts.iter().map(|t| at(t)).collect();
    run(&facts, UsageBy::Block, 36_500, now, "UTC", None)
}

#[test]
fn a_block_starts_at_the_utc_hour_of_its_first_call() {
    let out = blocks(&["2026-08-01T10:47:00.000Z"], "2026-08-01T12:00:00.000Z");
    let b = out.rows[0].block.clone().unwrap_or_else(|| unreachable!());
    assert_eq!(b.start_iso, "2026-08-01T10:00:00.000Z");
    assert_eq!(b.end_iso, "2026-08-01T15:00:00.000Z");
}

#[test]
fn a_call_past_start_plus_5h_opens_the_next_block() {
    let out = blocks(
        &["2026-08-01T10:00:00.000Z", "2026-08-01T15:01:00.000Z"],
        NOW,
    );
    let starts: Vec<String> = block_starts(&out).into_iter().map(|b| b.0).collect();
    assert_eq!(
        starts,
        ["2026-08-01T10:00:00.000Z", "2026-08-01T15:00:00.000Z"]
    );
}

#[test]
fn a_gap_over_5h_opens_the_next_block_and_calls_sort_first() {
    let out = blocks(
        &[
            "2026-08-01T21:30:00.000Z",
            "2026-08-01T10:10:00.000Z",
            "2026-08-01T11:00:00.000Z",
        ],
        NOW,
    );
    let got = block_starts(&out);
    let turns: Vec<f64> = got.iter().map(|b| b.1).collect();
    assert_eq!(turns, [2.0, 1.0]);
    assert_eq!(got[1].0, "2026-08-01T21:00:00.000Z");
}

#[test]
fn a_block_is_active_while_open() {
    let calls = ["2026-08-01T10:47:00.000Z"];
    assert!(block_starts(&blocks(&calls, "2026-08-01T14:59:00.000Z"))[0].2);
    assert!(!block_starts(&blocks(&calls, "2026-08-01T15:00:00.000Z"))[0].2);
}

#[test]
fn an_unknown_zone_is_rejected() {
    use peektrace_core::stats::usage_service::zone_of;
    assert!(zone_of("Mars/Olympus").is_none());
}

#[test]
fn a_claude_dedup_caveat_does_not_name_claude() {
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
    assert!(!out.caveats.iter().any(|c| c.contains("Claude")));
}

fn turn_cost_of(f: &Fact) -> f64 {
    use peektrace_core::pricing::{CostTokens, card_for, turn_cost};
    let Fact::Usage(u) = f else { return 0.0 };
    turn_cost(
        card_for(&u.model).as_ref(),
        &CostTokens {
            input: u.input_tokens,
            output: u.output_tokens,
            cache_read: u.cache_read_tokens,
            cache_creation: u.cache_creation_tokens,
            cache_creation_1h: None,
        },
    )
    .unwrap_or(0.0)
}

fn day(facts: &[Fact]) -> UsageResult {
    run(facts, UsageBy::Day, 30, NOW, "UTC", None)
}

#[test]
fn a_mixed_model_day_sums_each_model_at_its_rates() {
    use peektrace_core::stats::rank::round;
    let ts = "2026-08-05T10:00:00.000Z";
    let opus = with(at(ts), |u| {
        u.model = "claude-opus-4-1".into();
        u.input_tokens = 1_000_000.0;
    });
    let sonnet = with(at(ts), |u| {
        u.input_tokens = 0.0;
        u.output_tokens = 1_000_000.0;
    });
    let want = round(turn_cost_of(&opus) + turn_cost_of(&sonnet));
    let out = day(&[opus, sonnet]);
    assert_eq!(out.rows[0].totals.agg.usd, want);
    assert_eq!(
        out.rows[0].totals.models,
        ["claude-opus-4-1", "claude-sonnet-4-5"]
    );
    assert_eq!(out.totals.agg.usd, want);
    assert_eq!(out.totals.total_tokens, 2_000_050.0);
    assert_eq!(out.unpriced_models, Vec::<String>::new());
}

#[test]
fn a_long_prompt_bills_at_the_long_tier() {
    use peektrace_core::stats::rank::round;
    let ts = "2026-08-05T10:00:00.000Z";
    let haiku = |input: f64| {
        with(at(ts), |u| {
            u.model = "claude-haiku-5-5".into();
            u.input_tokens = input;
        })
    };
    assert_eq!(
        day(&[haiku(99_000.0)]).totals.agg.usd,
        round(0.0099 + 0.000_025)
    );
    assert_eq!(
        day(&[haiku(101_000.0)]).totals.agg.usd,
        round(0.0505 + 0.000_125)
    );
}

#[test]
fn an_unknown_model_adds_no_cost_and_is_named() {
    use peektrace_core::stats::rank::round;
    let ts = "2026-08-05T10:00:00.000Z";
    let priced = at(ts);
    let want = round(turn_cost_of(&priced));
    let out = day(&[
        priced,
        with(at(ts), |u| {
            u.model = "made-up-model".into();
            u.agent = Agent::Codex;
        }),
    ]);
    assert_eq!(out.unpriced_models, ["made-up-model"]);
    assert_eq!(out.totals.agg.usd, want);
    assert!(
        out.caveats
            .iter()
            .any(|c| c.contains("made-up-model") && c.contains("left out of the cost"))
    );
}

#[test]
fn zero_token_and_untimed_calls_are_counted_out() {
    let ts = "2026-08-05T10:00:00.000Z";
    let out = day(&[
        with(at(ts), |u| {
            u.input_tokens = 0.0;
            u.output_tokens = 0.0;
        }),
        with(at(ts), |u| u.ts = None),
        at(ts),
        Fact::Session(common::session_fact()),
        Fact::Call(common::tool_fact()),
    ]);
    assert_eq!(out.totals.agg.turns, 1.0);
    assert_eq!(out.skipped.zero_tokens, 1.0);
    assert_eq!(out.skipped.no_timestamp, 1.0);
    assert!(out.caveats.iter().any(|c| c.contains("no timestamp")));
}

#[test]
fn the_report_detectors_ignore_usage_rows() {
    use peektrace_core::stats::accumulate::{CorpusSummary, CorpusTotals};
    use peektrace_core::stats::detectors::corpus::CorpusPanelSink;
    use peektrace_core::stats::detectors::corpus_rows::CorpusPanels;
    use peektrace_core::stats::redact::Redactor;
    use peektrace_core::stats::schema::Kind;
    let mut s = common::session_fact();
    s.base.run = Some("wf_1".into());
    s.base.kind = Kind::WorkflowAgent;
    let mut t = common::tool_fact();
    t.base.run = Some("wf_1".into());
    t.base.kind = Kind::WorkflowAgent;
    let base = vec![Fact::Session(s), Fact::Call(t)];
    let mut with_usage = base.clone();
    with_usage.push(Fact::Usage(common::usage_fact()));
    let mut second = common::usage_fact();
    second.req = "req_2".into();
    with_usage.push(Fact::Usage(second));
    let fold = |facts: &[Fact]| -> (CorpusSummary, CorpusPanels) {
        let mut totals = CorpusTotals::default();
        let mut panels = CorpusPanelSink::default();
        for f in facts {
            totals.add(f);
            panels.add(f);
        }
        let (corpus, window, redact) = (
            CorpusSummary::default(),
            common::window(),
            Redactor::new(true),
        );
        let p = panels.finish(&mut common::ctx_of(&corpus, &window, &redact));
        (totals.finish(), p)
    };
    let got = fold(&with_usage);
    assert_eq!(got, fold(&base));
    assert_eq!(got.0.tool_calls, 1.0);
}

mod common;
