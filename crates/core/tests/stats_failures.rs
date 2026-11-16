//! The output scanner, the benign filters and the fail table, plus the lint
//! pattern against the TS one it replaces.
#![allow(clippy::float_cmp, reason = "counts are whole numbers")]

mod common;

use common::{bash_fact, bash_mut, call, ctx_of, window};
use peektrace_core::js::regex::Re;
use peektrace_core::stats::accumulate::{CorpusSummary, DrillTable, DrillTarget};
use peektrace_core::stats::detectors::causes::detect_cause;
use peektrace_core::stats::detectors::failures::{
    FailRow, FailTable, LINT_RE, benign_filter, is_piped, scan_output,
};
use peektrace_core::stats::redact::Redactor;
use peektrace_core::stats::schema::{CallFact, CauseId};

fn shell(text: &str) -> Option<&'static str> {
    scan_output("Bash", text, false).reason
}

#[test]
fn passing_bun_test_run_is_not_a_failure() {
    let s = scan_output("Bash", " 24 pass\n 0 fail\n", false);
    assert_eq!((s.reason, s.failed), (None, false));
}

#[test]
fn non_zero_failing_count_is_a_failure() {
    assert_eq!(shell(" 21 pass\n 3 fail\n"), Some("test"));
    assert_eq!(shell("Tests  2 failed | 8 passed"), Some("test"));
    assert_eq!(shell("FAIL  src/thing.test.ts > case"), Some("test"));
}

#[test]
fn tsc_diagnostic_fires_bare_word_does_not() {
    assert_eq!(shell("src/x.ts(12,3): error TS2339: nope"), Some("tsc"));
    assert_eq!(shell("error TS: something went wrong"), None);
}

#[test]
fn lint_needs_file_line_col_or_summary() {
    assert_eq!(shell("src/x.ts:3:1 lint/style/useConst"), Some("lint"));
    assert_eq!(shell("Found 4 errors."), Some("lint"));
    assert_eq!(shell("the workflow writes to lint/ and exits"), None);
}

#[test]
fn traceback_and_node_frame_fire() {
    assert_eq!(
        shell("Traceback (most recent call last):"),
        Some("traceback")
    );
    assert_eq!(shell("boom\n    at run (/a/b.ts:12:3)"), Some("node"));
}

#[test]
fn scanner_only_reads_shell_output() {
    let read = scan_output("Read", "Traceback (most recent call last):", false);
    assert_eq!((read.reason, read.failed), (None, false));
}

#[test]
fn agents_own_flag_always_fails_the_row() {
    assert!(scan_output("Bash", "all good", true).failed);
}

#[test]
fn pipe_into_a_filter_discards_the_exit_code() {
    assert!(is_piped("bun run typecheck | tail -30"));
    assert!(is_piped("bun run typecheck 2>&1 | head"));
    assert!(!is_piped("bun run typecheck"));
}

fn filter_of(cmd: &str, output: &str) -> Option<&'static str> {
    benign_filter(cmd, output, false)
}

#[test]
fn benign_filters() {
    assert_eq!(
        filter_of(
            "grep -rn x packages/**/__fixtures__",
            "Exit code 1\nzsh: no matches found: packages/**/__fixtures__"
        ),
        Some("glob-no-match")
    );
    assert_eq!(
        filter_of("brew install x; which 7z", "Exit code 1\n7z not found"),
        Some("absence-probe")
    );
    let listing = format!("Exit code 1\n{}", "listing output line\n".repeat(30));
    assert_eq!(
        filter_of("ls -la; git status; false", &listing),
        Some("chained-tail")
    );
    let real = format!(
        "Exit code 1\n{}\nTraceback (most recent call last):",
        "x".repeat(240)
    );
    assert_eq!(filter_of("ls -la; python3 run.py", &real), None);
    assert_eq!(
        filter_of(
            "rm -rf /",
            "The user doesn't want to proceed with this tool use. The tool use was rejected."
        ),
        Some("user-rejected")
    );
    assert_eq!(
        filter_of(
            "rm -rf /",
            "The user doesn't want to take this action right now."
        ),
        Some("user-rejected")
    );
    let silent = "printed work\n".repeat(30);
    assert_eq!(
        filter_of("bun run typecheck | tail -5; true", &silent),
        None
    );
}

#[test]
fn a_claimed_row_is_never_suppressed() {
    let output = format!("Exit code 1\n{}", "printed work\n".repeat(30));
    assert_eq!(
        benign_filter("ls -la; echo ===; git status; false", &output, true),
        None
    );
    assert_eq!(
        benign_filter(
            "grep -rn x . --include=*.ts",
            "Exit code 1\n(eval):1: no matches found: --include=*.ts",
            true
        ),
        None
    );
}

fn fact(seq: f64, cmd: &str, edit: impl FnOnce(&mut CallFact)) -> CallFact {
    let mut f = bash_fact(cmd);
    f.seq = seq;
    edit(&mut f);
    f
}

fn rows_for(facts: Vec<CallFact>) -> (Vec<FailRow>, Vec<String>) {
    let mut table = FailTable::new(None);
    for f in facts {
        table.add(&call(f));
    }
    let (corpus, window, redact) = (CorpusSummary::default(), window(), Redactor::new(true));
    let mut ctx = ctx_of(&corpus, &window, &redact);
    let rows = table.finish(&mut ctx);
    (rows, ctx.caveats)
}

#[test]
fn a_failing_cause_owns_its_call() {
    let failed = |f: &mut CallFact, err: bool, cause: Option<CauseId>| {
        f.err = err;
        f.failed = true;
        f.cause = cause;
    };
    let (rows, _) = rows_for(vec![
        fact(0.0, "ls; echo ===; git status", |f| {
            failed(f, true, Some(CauseId::ZshEqualsAbort));
        }),
        fact(1.0, "grep -rn x . --include=*.ts", |f| {
            failed(f, true, Some(CauseId::ZshGlobFlag));
        }),
        fact(2.0, "grep -rn y . --include=*.md | head", |f| {
            failed(f, false, Some(CauseId::ZshGlobFlag));
        }),
        fact(3.0, "bun test", |f| failed(f, true, None)),
    ]);
    let keys: Vec<(&str, f64)> = rows.iter().map(|r| (r.key.as_str(), r.rows)).collect();
    assert_eq!(keys, [("flag:Bash", 1.0)]);
    assert_eq!(rows[0].note, None);
}

#[test]
fn a_waste_cause_is_not_a_fail_row() {
    let (rows, _) = rows_for(vec![fact(0.0, "true", |f| {
        f.cause = Some(CauseId::OutputTooLarge);
        f.cause_arg = Some("b1.txt".into());
    })]);
    assert_eq!(rows, []);
}

#[test]
fn a_silent_failure_names_the_pipe() {
    let tsc = |f: &mut CallFact| {
        f.failed = true;
        f.fail_reason = Some("tsc".into());
        bash_mut(f).piped = true;
    };
    let (rows, _) = rows_for(vec![
        fact(0.0, "bun run typecheck | tail -30", tsc),
        fact(1.0, "bun run typecheck | head", |f| {
            tsc(f);
            f.base.sid = "s2".into();
        }),
    ]);
    let r = &rows[0];
    assert_eq!(r.key, "scan:tsc");
    assert_eq!(r.label, "error TS####");
    assert_eq!(r.r#where, "Bash exit 0");
    assert_eq!(r.flagged, 0.0);
    assert_eq!(r.detector.as_deref(), Some("piped-exit-code"));
    assert_eq!(
        r.note.as_deref(),
        Some("2 of 2 pipe into head/tail/grep, so the exit code is the pipe's")
    );
}

#[test]
fn a_suppressed_row_is_counted_in_a_caveat() {
    let (rows, caveats) = rows_for(vec![
        fact(0.0, "true", |f| {
            f.err = true;
            f.failed = true;
            f.benign = Some("glob-no-match".into());
        }),
        fact(1.0, "bun test", |f| {
            f.err = true;
            f.failed = true;
        }),
    ]);
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].key.as_str(), rows[0].rows), ("flag:Bash", 1.0));
    assert!(caveats.iter().any(|c| c.contains("1 of 2")), "{caveats:?}");
}

#[test]
fn rows_rank_by_sessions_then_rows() {
    let scan = |sid: String, reason: &'static str| {
        move |f: &mut CallFact| {
            f.base.sid = sid;
            f.failed = true;
            f.fail_reason = Some(reason.to_owned());
        }
    };
    let mut facts: Vec<CallFact> = (0..5)
        .map(|i| {
            fact(
                f64::from(i),
                "bun run typecheck",
                scan("deep".into(), "tsc"),
            )
        })
        .collect();
    facts.extend((0..3).map(|i| {
        fact(
            f64::from(i),
            "bun x ultracite check",
            scan(format!("wide{i}"), "lint"),
        )
    }));
    let (rows, _) = rows_for(facts);
    let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, ["scan:lint", "scan:tsc"]);
}

#[test]
fn drill_returns_the_calls_behind_one_key_chronologically() {
    let mut table = FailTable::new(Some(DrillTarget {
        table: DrillTable::Fail,
        key: "scan:tsc".into(),
    }));
    let at = |seq: f64, ts: &str, reason: &str, cmd: &str| {
        let ts = ts.to_owned();
        let reason = reason.to_owned();
        fact(seq, cmd, move |f| {
            if !ts.is_empty() {
                f.ts = Some(ts);
            }
            f.failed = true;
            f.fail_reason = Some(reason);
        })
    };
    table.add(&call(at(1.0, "2026-08-02T00:00:00.000Z", "tsc", "b")));
    table.add(&call(at(0.0, "2026-08-01T00:00:00.000Z", "tsc", "a")));
    table.add(&call(at(2.0, "", "lint", "true")));
    let (corpus, window, redact) = (CorpusSummary::default(), window(), Redactor::new(true));
    let hits = table.drill(&ctx_of(&corpus, &window, &redact));
    let lines: Vec<&str> = hits.iter().map(|h| h.sample.line.as_str()).collect();
    assert_eq!(lines, ["a", "b"]);
}

/// The labelled oracle (161 hand-labelled errored Bash rows of private
/// projects). Runs only when `PEEKTRACE_BENIGN_SAMPLE` names the file.
#[test]
fn benign_filters_against_the_labelled_sample() {
    let Some(path) = std::env::var("PEEKTRACE_BENIGN_SAMPLE")
        .ok()
        .filter(|p| !p.is_empty() && std::path::Path::new(p).exists())
    else {
        return;
    };
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let sample: Vec<serde_json::Value> = text
        .trim()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let s = |v: &serde_json::Value, k: &str| v[k].as_str().unwrap_or("").to_owned();
    let scored: Vec<(&serde_json::Value, Option<&str>)> = sample
        .iter()
        .map(|r| (r, benign_filter(&s(r, "cmd"), &s(r, "err_text"), false)))
        .collect();
    let label = |rows: &[&(&serde_json::Value, Option<&str>)], l: &str| {
        rows.iter().filter(|x| s(x.0, "label") == l).count()
    };
    let all: Vec<_> = scored.iter().collect();
    assert_eq!(sample.len(), 161);
    assert_eq!(label(&all, "BENIGN"), 59);
    assert_eq!(label(&all, "REAL"), 100);
    let suppressed: Vec<_> = scored.iter().filter(|x| x.1.is_some()).collect();
    assert_eq!(suppressed.len(), 50);
    assert_eq!(
        (label(&suppressed, "BENIGN"), label(&suppressed, "REAL")),
        (38, 11)
    );
    let per = |id: &str| {
        let hits: Vec<_> = suppressed
            .iter()
            .copied()
            .filter(|x| x.1 == Some(id))
            .collect();
        (hits.len(), label(&hits, "BENIGN"), label(&hits, "REAL"))
    };
    assert_eq!(per("glob-no-match"), (9, 9, 0));
    assert_eq!(per("user-rejected"), (3, 3, 0));
    assert_eq!(per("absence-probe"), (10, 9, 1));
    assert_eq!(per("chained-tail"), (28, 17, 10));
    let claimed = |r: &serde_json::Value| {
        detect_cause("Bash", &s(r, "cmd"), &s(r, "err_text"), &|x: &str| {
            x.to_owned()
        })
        .is_some()
    };
    let separator = Re::new(r"(^|[\s;&|])echo\s+=", "");
    let detected: Vec<_> = sample
        .iter()
        .filter(|r| separator.is_match(&s(r, "cmd")))
        .collect();
    assert_eq!(detected.len(), 30);
    assert!(detected.iter().all(|r| s(r, "label") == "REAL"));
    assert!(detected.iter().all(|r| claimed(r)));
    assert!(
        scored
            .iter()
            .filter(|x| x.1 == Some("glob-no-match"))
            .all(|x| claimed(x.0))
    );
}

/// The TS pattern, with its lookbehind anchor, as the oracle.
const TS_LINT: &str = r"(?:(?<![\w./-])[\w./-]+:\d+:\d+\s+lint\/[a-z]+\/\w+|\bFound \d+ errors?\b)";

fn match_of(re: &Re, s: &str) -> Option<(usize, String)> {
    re.find(s).map(|r| (r.start, s[r].to_owned()))
}

#[test]
fn lint_long_runs_scan_in_linear_time() {
    for text in [
        format!("{} lint/a/b", "x".repeat(100_000)),
        "a1:".repeat(30_000),
        format!("{}:1:2 lint", "-./".repeat(30_000)),
    ] {
        let start = std::time::Instant::now();
        let _ = shell(&text);
        assert!(start.elapsed().as_millis() < 200);
    }
    assert_eq!(
        shell(&format!("{}:1:2 lint/style/useConst", "x".repeat(50_000))),
        Some("lint")
    );
}

#[test]
fn lint_hand_samples_match_the_ts_pattern() {
    let oracle = Re::new(TS_LINT, "");
    for s in [
        "src/x.ts:3:1 lint/style/useConst  FIXABLE",
        "packages/a-b/c.d.ts:120:44 lint/suspicious/noExplicitAny ━━━",
        "Checked 40 files in 12ms. Found 3 errors.",
        "Found 1 error",
        "my dir/file.ts:1:2 lint/a/b",
        "a.ts:1:1 lint/x/y at offset zero",
        "prefix.src/x.ts:9:9 lint/a/b",
        "prefix/src/x.ts:9:9 lint/a/b",
        "prefix-src/x.ts:9:9 lint/a/b",
        "(src/x.ts:9:9 lint/a/b)",
        "x.ts:1:2\tlint/a/b",
        "x.ts:1:2\nlint/a/b",
        "the workflow writes to lint/ and exits",
        "nothing to see here",
        "",
    ] {
        assert_eq!(match_of(&LINT_RE, s), match_of(&oracle, s), "{s:?}");
    }
}

#[test]
fn lint_random_token_strings_match_the_ts_pattern() {
    const TOKENS: [&str; 16] = [
        "x",
        ".",
        "/",
        "-",
        ":",
        "1",
        " ",
        "\n",
        "lint/",
        "a/",
        "b",
        "Found",
        "errors",
        ":1:2 ",
        "lint/a/b",
        "Found 2 errors",
    ];
    let oracle = Re::new(TS_LINT, "");
    let mut a: u64 = 83;
    let mut rand = move || {
        a = (a * 16_807) % 2_147_483_647;
        #[allow(clippy::cast_precision_loss, reason = "below 2^31")]
        let v = (a - 1) as f64 / 2_147_483_646.0;
        v
    };
    let mut hits = 0;
    for _ in 0..5000 {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "small non-negative"
        )]
        let len = (rand() * 41.0).floor() as usize;
        let mut s = String::new();
        for _ in 0..len {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "small non-negative"
            )]
            let i = (rand() * 16.0).floor() as usize;
            s.push_str(TOKENS[i]);
        }
        let want = match_of(&oracle, &s);
        if want.is_some() {
            hits += 1;
        }
        assert_eq!(match_of(&LINT_RE, &s), want, "{s:?}");
    }
    assert!(hits > 0);
}
