//! The difflib port against golden ratios Python computed, and the hybrid
//! modes that compare only the failing segment.
#![allow(clippy::float_cmp, reason = "golden ratios compare exactly")]

use peektrace_core::stats::detectors::similarity::{
    FOCUS_SEP, FocusMode, SimilarityInput, hybrid_similarity, norm_seg, ratio, segments,
};

const GOLDEN: &str = include_str!("../../../packages/core/test/fixtures/stats/difflib-golden.json");

fn hybrid(
    text: &str,
    focus: Option<&str>,
    mode: FocusMode,
    shell: bool,
    retry: &str,
) -> (f64, String) {
    let failed = SimilarityInput {
        focus: focus.map(str::to_owned),
        mode,
        shell,
        text: text.to_owned(),
    };
    let hit = hybrid_similarity(&failed, retry, 0.0, &segments);
    (hit.score, hit.segment)
}

#[test]
fn ratio_agrees_with_difflib_on_every_golden_pair() {
    let cases: Vec<serde_json::Value> = serde_json::from_str(GOLDEN).expect("golden json");
    assert!(cases.len() >= 28);
    assert!(
        cases
            .iter()
            .any(|g| g["b"].as_str().unwrap_or("").encode_utf16().count() >= 200)
    );
    for g in &cases {
        let (a, b) = (g["a"].as_str().unwrap_or(""), g["b"].as_str().unwrap_or(""));
        let r = g["r"].as_f64().unwrap_or(-1.0);
        assert!((ratio(a, b) - r).abs() < 1e-9, "{a:?} {b:?}");
    }
}

#[test]
fn autojunk_changes_the_score_once_b_reaches_200() {
    let a = "ab".repeat(100);
    assert_eq!(ratio(&a[..199], &a[..199]), 1.0);
    assert!(ratio(&format!("x{a}"), &a) < 0.1);
}

#[test]
fn segments_split_drop_noise_and_strip_quotes() {
    assert_eq!(
        segments("cd /p && grep -rn 'a\\|b' --include='*.ts' . | head -5; echo x"),
        ["grep -rn a\\", "b --include=*.ts ."]
    );
    assert_eq!(segments("echo hi"), ["echo hi"]);
    assert_eq!(norm_seg(" grep  'x'   y "), "grep x y");
}

#[test]
fn glob_mode_compares_only_the_failing_segment() {
    let text = "grep -rn foo --include=*.ts . | head; git log --oneline -30";
    let retry = "grep -rn foo --include='*.ts' .";
    let (score, _) = hybrid(
        text,
        Some("grep -rn foo --include=*.ts ."),
        FocusMode::Glob,
        true,
        retry,
    );
    assert_eq!(score, 1.0);
    assert!(ratio(text, retry) < 0.7);
}

#[test]
fn cnf_mode_tries_the_retry_without_its_binary() {
    let (score, segment) = hybrid(
        "jq .name package.json",
        Some(".name package.json"),
        FocusMode::Cnf,
        true,
        "bun .name package.json",
    );
    assert_eq!(score, 1.0);
    assert_eq!(segment, "bun .name package.json");
}

#[test]
fn sleep_mode_accepts_a_retry_running_the_blocked_command() {
    let (score, _) = hybrid(
        "sleep 90; tail -5 /tmp/out.log",
        Some("tail -5 /tmp/out.log"),
        FocusMode::Sleep,
        true,
        "until grep -q DONE /tmp/out.log; do sleep 2; done",
    );
    assert_eq!(score, 0.9);
}

#[test]
fn whole_mode_and_non_shell_tools_compare_the_whole_text() {
    let (score, _) = hybrid(
        "/repo/src/a.ts",
        None,
        FocusMode::Whole,
        false,
        "/repo/src/a.ts",
    );
    assert_eq!(score, 1.0);
    let focus = format!("ls a*{FOCUS_SEP}cat b*");
    let (score, _) = hybrid("x", Some(&focus), FocusMode::Glob, true, "cat b*");
    assert_eq!(score, 1.0);
}
