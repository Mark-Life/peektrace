//! `packages/rpc/test/stats.test.ts` over `/api`, with the server clock
//! pinned so two reports over one corpus compare byte for byte. The
//! progress cases have no HTTP form, so they call the core over the same
//! fixture.

mod common;

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use common::stats_fixture::{self, FIXED_MS, SECRET, SID};
use common::{Options, TestServer, start};
use peektrace_core::js::time::Utc;
use peektrace_core::stats::cache::StatsCache;
use peektrace_core::stats::prompts::{PromptTarget, stats_prompt};
use peektrace_core::stats::service::{ReportRequest, StatsEnv, markers, refresh, report};
use serde_json::{Value, json};

/// A window wide enough to hold the fixture.
const WIDE: &str = "days=3650";

struct Fixture {
    server: TestServer,
    root: PathBuf,
    cache: String,
}

fn fixture(name: &str) -> Fixture {
    let root = common::temp_dir(&format!("stats-{name}"));
    stats_fixture::build(&root);
    // Outside the projects root, which the walk reads.
    let cache = common::temp_dir(&format!("stats-{name}-pt"))
        .join("stats-rs")
        .to_string_lossy()
        .into_owned();
    let server = start(&Options {
        name,
        roots: Some(stats_fixture::roots(&root)),
        stats_dir: Some(cache.clone()),
        now: Some(Arc::new(|| FIXED_MS)),
        ..Options::default()
    });
    Fixture {
        server,
        root,
        cache,
    }
}

fn get(f: &Fixture, path: &str) -> Value {
    let a = f.server.get(path);
    assert_eq!(a.status, 200, "{path}: {}", a.body);
    a.json()
}

fn find<'a>(rows: &'a Value, key: &str, value: &str) -> &'a Value {
    rows.as_array()
        .and_then(|r| r.iter().find(|x| x[key] == value))
        .unwrap_or(&Value::Null)
}

fn tag(v: &Value) -> &str {
    v["error"]["_tag"].as_str().unwrap_or_default()
}

// describe("stats.report")

#[test]
fn returns_the_versioned_envelope_over_the_fixture_corpus() {
    let f = fixture("envelope");
    let r = get(&f, &format!("/api/stats/report?{WIDE}"));
    assert_eq!(r["schemaVersion"], "peektrace-stats/v2");
    assert_eq!(r["generatedAt"].as_f64(), Some(FIXED_MS));
    assert_eq!(r["window"]["days"], 3650);
    assert_eq!(r["corpus"]["transcripts"], 2);
    assert_eq!(r["corpus"]["bashCalls"], 4);
    assert!(!r["caveats"].as_array().is_none_or(Vec::is_empty));
}

#[test]
fn a_subagents_own_model_calls_are_costed() {
    let f = fixture("subagent");
    let r = get(&f, &format!("/api/stats/report?{WIDE}"));
    let sub = find(&r["cost"], "key", "kind:subagent");
    assert_eq!(sub["inputTokens"], 500);
    assert_eq!(sub["cacheReadTokens"], 400);
    assert_eq!(sub["outputTokens"], 100);
    let caveats = r["caveats"].as_array().cloned().unwrap_or_default();
    assert!(
        caveats
            .iter()
            .any(|c| c.as_str().is_some_and(|c| c.contains("subagent")))
    );
}

#[test]
fn ranks_the_aborted_separator_and_the_silent_typecheck() {
    let f = fixture("rank");
    let r = get(&f, &format!("/api/stats/report?{WIDE}"));
    let rule = find(&r["causes"], "cause", "zsh-equals-abort");
    assert_eq!(rule["hits"], 1);
    assert_eq!(rule["sessions"], 1);
    assert!(
        rule["fix"]
            .as_str()
            .is_some_and(|s| s.contains("echo \"===\""))
    );
    let fails = r["fails"].as_array().cloned().unwrap_or_default();
    assert!(
        !fails
            .iter()
            .any(|x| x["key"].as_str().is_some_and(|k| k.starts_with("cause:")))
    );
    let silent = find(&r["fails"], "key", "scan:tsc");
    assert_eq!(silent["rows"], 1);
    assert_eq!(silent["flagged"], 0);
    assert_eq!(silent["detector"], "piped-exit-code");
}

#[test]
fn decodes_the_cause_rows_with_the_retry_pair_and_the_rule_check() {
    let f = fixture("causes");
    let r = get(&f, &format!("/api/stats/report?{WIDE}"));
    let glob = find(&r["causes"], "cause", "zsh-glob-flag");
    assert_eq!(glob["key"], "cause:zsh-glob-flag");
    assert_eq!(glob["hits"], 1);
    assert_eq!(glob["flagged"], 0);
    assert_eq!(glob["pairs"], 1);
    assert_eq!(glob["retrySec"], 6);
    assert_eq!(glob["retryCtxTokens"], 2010);
    assert_eq!(glob["diffs"][0]["to"], "--include='*.ts'");
    let m = &glob["rule"]["matches"][0];
    assert!(
        m["path"]
            .as_str()
            .is_some_and(|p| p.ends_with("/home/CLAUDE.md"))
    );
    assert_eq!(m["line"], 2);
    assert_eq!(find(&r["causes"], "cause", "zsh-equals-abort")["hits"], 1);
    assert_eq!(find(&r["fails"], "key", "flag:Bash"), &Value::Null);
}

#[test]
fn every_ranking_is_capped_and_every_truncation_is_stated() {
    let f = fixture("cap");
    let r = get(&f, &format!("/api/stats/report?{WIDE}&limit=1"));
    assert_eq!(r["fails"].as_array().map(Vec::len), Some(1));
    let caveats = r["caveats"].as_array().cloned().unwrap_or_default();
    assert!(
        caveats
            .iter()
            .any(|c| c.as_str().is_some_and(|c| c.contains("not shown")))
    );
}

#[test]
fn no_command_secret_survives_into_the_report() {
    let f = fixture("secret");
    let a = f.server.get(&format!("/api/stats/report?{WIDE}"));
    assert!(!a.body.contains(SECRET));
}

#[test]
fn a_sample_names_its_call_time_and_its_transcripts_path() {
    let f = fixture("sample");
    let r = get(&f, &format!("/api/stats/report?{WIDE}"));
    let failed = &find(&r["causes"], "cause", "zsh-glob-flag")["pairSamples"][0]["failed"];
    assert_eq!(failed["ts"], "2026-06-01T10:00:30.000Z");
    assert_eq!(failed["toolUseId"], "tool-3");
    let path = stats_fixture::transcript(&f.root);
    assert_eq!(r["paths"], json!({ SID: path.to_string_lossy() }));
}

#[test]
fn the_investigation_prompts_carry_the_paths_and_no_secret() {
    let f = fixture("prompts");
    let body = f.server.get(&format!("/api/stats/report?{WIDE}")).body;
    // The prompt is built client side from the report: build the same report
    // in core and check it is the body the route sent.
    let roots = stats_fixture::roots(&f.root);
    let env = StatsEnv {
        roots: &roots,
        cache: StatsCache::new(f.cache.clone()),
        local: &Utc,
        now_ms: FIXED_MS,
        progress: None,
    };
    let req = ReportRequest {
        days: Some(3650.0),
        ..ReportRequest::default()
    };
    let r = report(&env, &req).expect("report");
    assert_eq!(r.to_json(false), body);
    let texts: Vec<String> = [
        PromptTarget::Report,
        PromptTarget::Cause("zsh-glob-flag".to_owned()),
        PromptTarget::Fail("scan:tsc".to_owned()),
    ]
    .iter()
    .map(|t| stats_prompt(&r, None, t).unwrap_or_default())
    .collect();
    let path = stats_fixture::transcript(&f.root);
    assert!(texts[1].contains(&format!("· tool-3 · {}", path.to_string_lossy())));
    assert!(texts[2].contains("Pattern: error TS####"));
    assert!(texts.iter().all(|t| !t.contains(SECRET)));
}

#[test]
fn two_runs_over_one_corpus_print_byte_identical_json() {
    let f = fixture("twice");
    let path = format!("/api/stats/report?{WIDE}");
    let first = f.server.get(&path).body;
    let second = f.server.get(&path).body;
    assert_eq!(first, second);
}

#[test]
fn redact_false_and_bad_params_are_refused() {
    let f = fixture("bad-params");
    for q in [
        "redact=false",
        "agent=all",
        "agent=bogus",
        "days=abc",
        "limit=",
        "force=yes",
    ] {
        let a = f.server.get(&format!("/api/stats/report?{q}"));
        assert_eq!(a.status, 400, "{q}");
        assert_eq!(tag(&a.json()), "BadRequestError", "{q}");
    }
}

#[test]
fn a_repeated_agent_scopes_the_pass() {
    let f = fixture("agents");
    let codex = get(&f, &format!("/api/stats/report?{WIDE}&agent=codex"));
    assert_eq!(codex["corpus"]["transcripts"], 0);
    let both = get(
        &f,
        &format!("/api/stats/report?{WIDE}&agent=codex&agent=claude"),
    );
    assert_eq!(both["corpus"]["transcripts"], 2);
}

// describe("stats.usage")

#[test]
fn buckets_every_model_call_by_day_matching_the_reports_tokens() {
    let f = fixture("usage-day");
    let u = get(&f, "/api/stats/usage?by=day&days=60");
    let r = get(&f, &format!("/api/stats/report?{WIDE}"));
    let kinds: f64 = r["cost"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|x| x["key"].as_str().is_some_and(|k| k.starts_with("kind:")))
                .filter_map(|x| x["inputTokens"].as_f64())
                .sum()
        })
        .unwrap_or_default();
    assert_eq!(u["schemaVersion"], "peektrace-stats-usage/v1");
    assert_eq!(u["generatedAt"].as_f64(), Some(FIXED_MS));
    assert_eq!(u["tz"], "UTC");
    assert_eq!(u["rows"], json!([u["rows"][0].clone()]));
    assert_eq!(u["rows"][0]["key"], "2026-06-01");
    assert_eq!(u["totals"]["inputTokens"].as_f64(), Some(kinds));
    assert!(u["totals"]["usd"].as_f64().is_some_and(|x| x > 0.0));
}

#[test]
fn blocks_open_at_the_utc_hour_of_the_first_call() {
    let f = fixture("usage-block");
    let u = get(&f, "/api/stats/usage?by=block&days=60");
    assert_eq!(u["rows"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        u["rows"][0]["block"]["startIso"],
        "2026-06-01T10:00:00.000Z"
    );
    assert_eq!(u["rows"][0]["block"]["active"], false);
}

#[test]
fn a_second_usage_call_reads_the_cache_and_prints_the_same_bytes() {
    let f = fixture("usage-month");
    let first = f.server.get("/api/stats/usage?by=month&days=365").body;
    let second = f.server.get("/api/stats/usage?by=month&days=365").body;
    assert_eq!(first, second);
    let u: Value = serde_json::from_str(&first).expect("json");
    assert_eq!(u["rows"][0]["key"], "2026-06");
    assert_eq!(u["rows"].as_array().map(Vec::len), Some(1));
}

#[test]
fn an_unknown_time_zone_is_a_typed_scan_failure() {
    let f = fixture("usage-tz");
    let a = f.server.get("/api/stats/usage?by=day&tz=Mars/Olympus");
    assert_eq!(a.status, 500);
    assert_eq!(
        a.json(),
        json!({"error": {"_tag": "StatsScanError", "reason": "unknown time zone: Mars/Olympus"}})
    );
}

#[test]
fn usage_params_are_checked() {
    let f = fixture("usage-bad");
    for q in [
        "",
        "by=week",
        "by=day&days=0",
        "by=day&days=1.5",
        "by=day&days=36501",
    ] {
        let a = f.server.get(&format!("/api/stats/usage?{q}"));
        assert_eq!(a.status, 400, "{q}");
    }
}

// describe("stats.drill")

#[test]
fn expands_the_rows_behind_a_fail_key() {
    let f = fixture("drill-fail");
    let d = get(&f, &format!("/api/stats/drill/fail/scan:tsc?{WIDE}"));
    assert_eq!(d["schemaVersion"], "peektrace-stats/v2");
    assert_eq!(d["total"], 1);
    assert_eq!(d["hits"][0]["sid"], SID);
    assert_eq!(d["hits"][0]["failed"], true);
}

#[test]
fn expands_a_cause_into_its_failed_calls_and_their_retries() {
    let f = fixture("drill-cause");
    for key in ["cause:zsh-glob-flag", "zsh-glob-flag"] {
        let d = get(&f, &format!("/api/stats/drill/cause/{key}?{WIDE}"));
        assert_eq!(d["key"], "cause:zsh-glob-flag");
        assert_eq!(d["total"], 1);
        assert_eq!(d["hits"][0]["toolUseId"], "tool-3");
        assert_eq!(d["hits"][0]["pairToolUseId"], "tool-4");
        assert!(
            d["hits"][0]["line"]
                .as_str()
                .is_some_and(|l| !l.contains(" → "))
        );
        assert!(!d["hits"][0]["pairLine"].is_null());
    }
}

#[test]
fn a_page_states_where_it_starts_and_keeps_the_true_total() {
    let f = fixture("drill-page");
    let base = format!("/api/stats/drill/cause/cause:zsh-equals-abort?{WIDE}");
    let first = get(&f, &format!("{base}&pageSize=1"));
    assert_eq!(first["offset"], 0);
    assert_eq!(first["hits"].as_array().map(Vec::len), Some(1));
    let past = get(&f, &format!("{base}&offset=1&pageSize=1"));
    assert_eq!(past["offset"], 1);
    assert_eq!(past["hits"], json!([]));
    assert_eq!(past["paths"], json!({}));
    assert_eq!(past["total"], first["total"]);
    let clamped = get(&f, &format!("{base}&offset=-3.7&pageSize=0.2"));
    assert_eq!(clamped["offset"], 0);
    assert_eq!(clamped["hits"].as_array().map(Vec::len), Some(1));
}

#[test]
fn an_unknown_key_is_a_typed_wire_failure() {
    let f = fixture("drill-nope");
    let a = f.server.get(&format!("/api/stats/drill/fail/nope?{WIDE}"));
    assert_eq!(a.status, 404);
    assert_eq!(
        a.json(),
        json!({"error": {"_tag": "StatsRowNotFoundError", "key": "nope", "table": "fail"}})
    );
    let wait = f
        .server
        .get(&format!("/api/stats/drill/wait/wait%2Fpoll?{WIDE}"));
    assert_eq!(wait.status, 404);
    assert_eq!(wait.json()["error"]["key"], "wait/poll");
    let bogus = f.server.get("/api/stats/drill/bogus/x");
    assert_eq!(bogus.status, 400);
}

// describe("stats.markers")

#[test]
fn marks_the_same_calls_the_tables_rank_keyed_on_the_tool_use_id() {
    let f = fixture("markers");
    let m = get(&f, &format!("/api/sessions/{SID}/markers"));
    assert_eq!(m["schemaVersion"], "peektrace-stats/v2");
    assert_eq!(m["sid"], SID);
    let ids: Vec<(Value, Value)> = m["markers"]
        .as_array()
        .map(|v| {
            v.iter()
                .map(|x| (x["toolUseId"].clone(), x["detector"].clone()))
                .collect()
        })
        .unwrap_or_default();
    for (id, detector) in [
        ("tool-1", "zsh-equals-abort"),
        ("tool-2", "piped-exit-code"),
        ("tool-3", "zsh-glob-flag"),
        ("tool-4", "retry"),
    ] {
        assert!(
            ids.contains(&(json!(id), json!(detector))),
            "{id} {detector}"
        );
    }
    let codex = get(&f, &format!("/api/sessions/{SID}/markers?agent=codex"));
    assert_eq!(codex["markers"], json!([]));
}

#[test]
fn markers_answer_while_a_pass_holds_the_stats_lock() {
    let root = common::temp_dir("stats-markers-busy");
    stats_fixture::build(&root);
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    let server = start(&Options {
        name: "markers-busy",
        roots: Some(stats_fixture::roots(&root)),
        now: Some(Arc::new(|| FIXED_MS)),
        stats_lock: Some(Arc::clone(&lock)),
        ..Options::default()
    });
    let busy = lock.blocking_lock();
    let (tx, rx) = std::sync::mpsc::channel();
    let path = format!("/api/sessions/{SID}/markers");
    let answer = std::thread::scope(|s| {
        s.spawn(|| {
            let _ = tx.send(server.get(&path));
        });
        let answer = rx.recv_timeout(std::time::Duration::from_secs(10));
        drop(busy);
        answer
    });
    let m = answer.expect("markers waited for the stats lock");
    assert_eq!(m.status, 200, "{}", m.body);
    assert!(!m.json()["markers"].as_array().is_none_or(Vec::is_empty));
}

#[test]
fn an_unknown_session_marks_nothing() {
    let f = fixture("markers-none");
    let m = get(&f, "/api/sessions/no-such-session/markers");
    assert_eq!(m["markers"], json!([]));
}

// describe("stats progress")

fn progress_calls(name: &str, pass: impl Fn(&StatsEnv<'_>)) -> Vec<(usize, usize)> {
    let root = common::temp_dir(&format!("stats-{name}"));
    stats_fixture::build(&root);
    let cache = common::temp_dir(&format!("stats-{name}-pt")).join("stats-rs");
    let roots = stats_fixture::roots(&root);
    let calls = RefCell::new(Vec::new());
    let hook = |done: usize, total: usize| calls.borrow_mut().push((done, total));
    let env = StatsEnv {
        roots: &roots,
        cache: StatsCache::new(cache.to_string_lossy().into_owned()),
        local: &Utc,
        now_ms: FIXED_MS,
        progress: Some(&hook),
    };
    pass(&env);
    calls.into_inner()
}

#[test]
fn a_progress_hook_sees_every_transcript() {
    let calls = progress_calls("progress-all", |env| {
        refresh(env, None, true).expect("refresh");
    });
    assert_eq!(calls, vec![(1, 2), (2, 2)]);
}

#[test]
fn a_narrowed_pass_counts_only_its_own_transcript() {
    let calls = progress_calls("progress-one", |env| {
        markers(env, SID, None, false).expect("markers");
    });
    assert_eq!(calls, vec![(1, 1)]);
}

// describe("stats.refresh")

#[test]
fn re_extracts_every_transcript_under_force() {
    let f = fixture("refresh");
    let a = f
        .server
        .call("POST", "/api/stats/refresh", &[], Some(r#"{"force":true}"#));
    assert_eq!(a.status, 200, "{}", a.body);
    let s = a.json();
    assert_eq!(s["transcripts"], 2);
    assert_eq!(s["extracted"], 2);
    assert_eq!(s["cached"], 0);
    assert!(s["facts"].as_f64().is_some_and(|n| n > 0.0));
    let warm = f
        .server
        .call("POST", "/api/stats/refresh", &[], Some("{}"))
        .json();
    assert_eq!(warm["cached"], 2);
    assert_eq!(warm["extracted"], 0);
}

#[test]
fn reading_one_sessions_markers_leaves_the_rest_of_the_cache_alone() {
    let f = fixture("refresh-markers");
    assert_eq!(
        f.server
            .call("POST", "/api/stats/refresh", &[], None)
            .status,
        200
    );
    get(&f, "/api/sessions/no-such-session/markers");
    let after = f
        .server
        .call("POST", "/api/stats/refresh", &[], None)
        .json();
    assert_eq!(after["cached"], 2);
    assert_eq!(after["extracted"], 0);
}

#[test]
fn bad_refresh_bodies_are_bad_requests() {
    let f = fixture("refresh-bad");
    for body in [
        "[]",
        r#"{"force":"yes"}"#,
        r#"{"agents":["all"]}"#,
        r#"{"agents":"claude"}"#,
    ] {
        let a = f.server.call("POST", "/api/stats/refresh", &[], Some(body));
        assert_eq!(a.status, 400, "{body}");
    }
}
