//! The rule check: probes matched against memory lines, the covers and days
//! it asks about, and line dates from the ledger.
#![allow(clippy::float_cmp, reason = "dates and rates are exact")]

mod common;

use std::cell::RefCell;

use common::TempDir;
use common::rules::*;
use peektrace_core::Agent;
use peektrace_core::stats::detectors::causes::GIT_DIFF_FIX;
use peektrace_core::stats::detectors::rules::{
    CauseCounts, CoverSince, MemoryDoc, RowRef, RuleCheck, RuleCover, Split, check_row,
};
use peektrace_core::stats::redact::Redactor;
use peektrace_core::stats::rule_dates::{LineDates, LineLedger, date_lines};
use peektrace_core::stats::schema::CauseId;

type Asked = RefCell<Vec<Vec<(RuleCover, String)>>>;

/// Counts that reach every file and remember the covers the check asked about.
struct Recording<'a>(&'a Asked);

impl CauseCounts for Recording<'_> {
    fn since(&self, _key: &str, covers: &[CoverSince<'_>]) -> (Split, Split) {
        self.0.borrow_mut().push(
            covers
                .iter()
                .map(|c| (c.cover.clone(), c.after_day.clone()))
                .collect(),
        );
        (
            Split {
                main: 1.0,
                nested: 9.0,
            },
            Split {
                main: 100.0,
                nested: 400.0,
            },
        )
    }
}

/// Only Claude made calls: a Codex-only file reaches no hit.
struct ClaudeOnly;

impl CauseCounts for ClaudeOnly {
    fn since(&self, _key: &str, covers: &[CoverSince<'_>]) -> (Split, Split) {
        let n = if covers.iter().any(|c| c.cover.agent != Agent::Codex) {
            1.0
        } else {
            0.0
        };
        (
            Split {
                main: n,
                nested: 0.0,
            },
            Split {
                main: 100.0 * n,
                nested: 0.0,
            },
        )
    }
}

/// A file changed on the last day: hits only when no day bound is asked.
struct Fresh;

impl CauseCounts for Fresh {
    fn since(&self, _key: &str, covers: &[CoverSince<'_>]) -> (Split, Split) {
        if covers.iter().all(|c| c.after_day.is_empty()) {
            (
                Split {
                    main: 1.0,
                    nested: 0.0,
                },
                Split {
                    main: 10.0,
                    nested: 0.0,
                },
            )
        } else {
            (Split::default(), Split::default())
        }
    }
}

fn row(cause: CauseId) -> RowRef<'static> {
    RowRef {
        key: Box::leak(format!("cause:{}", cause.as_str()).into_boxed_str()),
        cause,
        fix: Some("quote"),
        details: &[],
    }
}

fn check(row: &RowRef<'_>, docs: &[MemoryDoc], counts: &dyn CauseCounts) -> Option<RuleCheck> {
    check_row(
        row,
        docs,
        counts,
        &LineDates::by_mtime(),
        &Redactor::new(true),
    )
}

fn any_counts() -> Recording<'static> {
    Recording(Box::leak(Box::default()))
}

fn one_doc(lines: &[&str]) -> Vec<MemoryDoc> {
    vec![MemoryDoc {
        display_path: "~/x.md".into(),
        real_path: "/x.md".into(),
        cover: cover(Agent::Claude, None),
        mtime_ms: None,
        lines: lines.iter().map(|l| (*l).to_owned()).collect(),
    }]
}

fn app_docs(t: &TempDir) -> Vec<MemoryDoc> {
    read_docs(t, &[t.join("u/proj/app")], ".claude", &CLAUDE_CODEX)
}

#[test]
fn a_probe_matching_one_line_wins_over_a_window() {
    let t = fixture();
    let asked = Asked::default();
    let rule = check(
        &row(CauseId::ZshGlobFlag),
        &app_docs(&t),
        &Recording(&asked),
    )
    .expect("rule");
    let lines: Vec<f64> = rule.matches.iter().map(|m| m.line).collect();
    assert_eq!(lines, [4.0]);
    assert_eq!(rule.since_iso.as_deref(), Some("2026-08-01T12:00:00.000Z"));
    assert_eq!(
        asked.borrow().last().cloned().unwrap_or_default(),
        [
            (cover(Agent::Claude, None), "2026-08-01".to_owned()),
            (cover(Agent::Codex, None), "2026-08-01".to_owned()),
        ]
    );
    assert_eq!(
        rule.per1k_since,
        Split {
            main: 10.0,
            nested: 22.5
        }
    );
    assert!(rule.advice.contains("setopt nonomatch"));
    assert!(rule.advice.contains("90% of hits come from subagents"));
}

#[test]
fn a_file_changed_on_the_last_day_says_what_to_do() {
    let t = fixture();
    let rule = check(&row(CauseId::ZshGlobFlag), &app_docs(&t), &Fresh).expect("rule");
    assert!(rule.advice.starts_with("if it keeps firing: "));
    assert!(rule.advice.contains("setopt nonomatch"));
}

#[test]
fn a_file_that_reaches_none_of_the_hits_does_not_count() {
    let docs = vec![MemoryDoc {
        cover: cover(Agent::Codex, None),
        display_path: "~/.codex/AGENTS.md".into(),
        real_path: "/c/AGENTS.md".into(),
        ..one_doc(&["- Quote every glob in a flag value."]).remove(0)
    }];
    assert!(check(&row(CauseId::ZshGlobFlag), &docs, &ClaudeOnly).is_none());
}

#[test]
fn flag_glob_line_is_not_the_path_rule_node_modules_is_not_a_module_rule() {
    let t = fixture();
    let docs = app_docs(&t);
    assert!(check(&row(CauseId::ZshGlobPath), &docs, &any_counts()).is_none());
    assert!(check(&row(CauseId::ScratchModuleResolution), &docs, &any_counts()).is_none());
}

#[test]
fn a_cause_with_no_matching_line_or_probe_has_no_rule() {
    let t = fixture();
    let docs = app_docs(&t);
    assert!(check(&row(CauseId::UgrepComplexity), &docs, &any_counts()).is_none());
    let schema = RowRef {
        key: "cause:x",
        ..row(CauseId::StructuredOutputSchema)
    };
    assert!(check(&schema, &docs, &any_counts()).is_none());
}

fn cnf_row(details: &'static [(String, f64)]) -> RowRef<'static> {
    RowRef {
        details,
        ..row(CauseId::CommandNotFound)
    }
}

fn details(bins: &[(&str, f64)]) -> &'static [(String, f64)] {
    Box::leak(
        bins.iter()
            .map(|(b, n)| ((*b).to_owned(), *n))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

#[test]
fn a_missing_binary_matches_only_a_line_saying_it_is_missing() {
    let t = fixture();
    let r = cnf_row(details(&[("jq", 3.0), ("file", 1.0)]));
    let rule = check(&r, &app_docs(&t), &any_counts()).expect("rule");
    let lines: Vec<f64> = rule.matches.iter().map(|m| m.line).collect();
    assert_eq!(lines, [2.0]);
}

#[test]
fn a_binary_name_too_long_to_compile_is_skipped() {
    let t = fixture();
    let long = "x".repeat(200_000);
    let r = cnf_row(details(&[(&long, 1.0), ("jq", 3.0)]));
    let rule = check(&r, &app_docs(&t), &any_counts()).expect("rule");
    let lines: Vec<f64> = rule.matches.iter().map(|m| m.line).collect();
    assert_eq!(lines, [2.0]);
}

#[test]
fn prose_saying_no_file_was_found_is_not_a_missing_binary_rule() {
    let r = cnf_row(details(&[("file", 1.0)]));
    let has = |line: &str| check(&r, &one_doc(&[line]), &any_counts()).is_some();
    assert!(!has("- If the search finds no file, say so."));
    assert!(has("- This box has no file: use stat."));
    assert!(has("- There is no `file` binary here."));
}

#[test]
fn a_binarys_name_must_be_a_whole_word() {
    let has = |bin: &str, line: &str| {
        check(
            &cnf_row(details(&[(bin, 1.0)])),
            &one_doc(&[line]),
            &any_counts(),
        )
        .is_some()
    };
    assert!(!has("file", "- Never install files with sudo; ask first."));
    assert!(!has(
        "file",
        "- Edit in place without filesystem-wide searches."
    ));
    assert!(!has(
        "node",
        "- Use bun instead of node_modules/.bin scripts."
    ));
    assert!(!has(
        "jq",
        "- Prefer python instead of jquery-style parsing."
    ));
    assert!(!has("bc", "- Do not install bcrypt globally."));
    assert!(!has("file", "- Never install file.txt fixtures."));
    assert!(has("jq", "- Use bun -e instead of jq."));
    assert!(has("rsync", "- Install `rsync` before syncing."));
}

/// Expected values are the TS probe's (lookbehind and lookahead) answers.
#[test]
fn a_binarys_word_bounds_match_the_ts_probe() {
    let r = cnf_row(details(&[("jq", 1.0)]));
    let has = |line: &str| check(&r, &one_doc(&[line]), &any_counts()).is_some();
    for (line, want) in [
        ("jq is not installed", true),
        ("x.jq is missing", false),
        ("-jq missing", false),
        ("(jq) missing", false),
        ("(jq missing", true),
        ("JQ IS MISSING", true),
        ("`jq` unavailable", true),
        ("install jq.", true),
        ("install jq.x", false),
        ("install jq, then", true),
        ("install jq", true),
        ("install jq)", true),
        ("no jq binary", true),
        ("no jqx binary", false),
        ("there is no jq.", true),
        ("has no `jq`:", true),
        ("no `jq`.x", false),
        ("ajq missing", false),
    ] {
        assert_eq!(has(line), want, "{line}");
    }
}

#[test]
fn a_loose_line_is_not_a_rule() {
    let has = |r: &RowRef<'_>, line: &str| check(r, &one_doc(&[line]), &any_counts()).is_some();
    let equals = row(CauseId::ZshEqualsAbort);
    let output = row(CauseId::OutputTooLarge);
    let path = row(CauseId::ZshGlobPath);
    assert!(!has(&equals, "- Use `===` and `!==`, never `==`."));
    assert!(has(&equals, "- Quote separator echoes: `echo \"===\"`."));
    assert!(!has(
        &output,
        "- Before committing, review with git diff HEAD."
    ));
    assert!(!has(
        &path,
        "- Use the Glob tool instead of find for file search."
    ));
    assert!(has(
        &path,
        "- Not `ls dir/*.x` or `(N)` globs: zsh aborts on them."
    ));
    let diff_line = "- Run `git diff --stat` first, then one file at a time.";
    assert!(!has(&output, diff_line));
    let git = RowRef {
        fix: Some(GIT_DIFF_FIX),
        ..row(CauseId::OutputTooLarge)
    };
    assert!(has(&git, diff_line));
    assert!(has(
        &output,
        "- Bound shell output: pipe it through | head -n 200."
    ));
}

#[test]
fn every_file_counts_from_the_newest_ones_day() {
    let doc = |path: &str, day: &str, dir: Option<&str>| MemoryDoc {
        display_path: path.into(),
        real_path: path.into(),
        cover: cover(Agent::Claude, dir),
        mtime_ms: Some(ms(&format!("{day}T12:00:00Z"))),
        lines: vec!["Quote every glob passed as a flag value.".into()],
    };
    let asked = Asked::default();
    let rule = check(
        &row(CauseId::ZshGlobFlag),
        &[
            doc("~/.claude/CLAUDE.md", "2026-10-01", None),
            doc("~/proj/CLAUDE.md", "2026-10-21", Some("/u/proj")),
        ],
        &Recording(&asked),
    )
    .expect("rule");
    assert_eq!(
        rule.since_iso.as_deref().map(|s| &s[..10]),
        Some("2026-10-21")
    );
    assert_eq!(rule.matches[0].path, "~/proj/CLAUDE.md");
    let days: Vec<String> = asked
        .borrow()
        .last()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|c| c.1)
        .collect();
    assert_eq!(days, ["2026-10-21", "2026-10-21"]);
}

#[test]
fn a_secret_on_a_matched_line_is_redacted() {
    let line = format!("Quote every glob; token {SECRET}");
    let rule = check(
        &row(CauseId::ZshGlobFlag),
        &one_doc(&[&line]),
        &any_counts(),
    )
    .expect("rule");
    assert!(!rule.matches[0].excerpt.contains(SECRET));
    assert_eq!(rule.since_iso, None);
}

#[test]
fn a_secret_straddling_the_clip_is_redacted_whole() {
    let lead = format!("Quote every glob; {} ", "x".repeat(160 - 18 - 1 - 16));
    let line = format!("{lead}{SECRET} end");
    let rule = check(
        &row(CauseId::ZshGlobFlag),
        &one_doc(&[&line]),
        &any_counts(),
    )
    .expect("rule");
    let excerpt = &rule.matches[0].excerpt;
    assert!(!excerpt.contains("sk-ant-"));
    assert!(excerpt.contains("[REDACTED:"));
}

const GLOB_LINE: &str = "- Quote every glob passed as a flag value.";

fn dated_doc(lines: &[&str], day: &str) -> MemoryDoc {
    MemoryDoc {
        display_path: "~/.agents/AGENTS.md".into(),
        real_path: "/u/.agents/AGENTS.md".into(),
        cover: cover(Agent::Claude, None),
        mtime_ms: Some(ms(&format!("{day}T12:00:00Z"))),
        lines: lines.iter().map(|l| (*l).to_owned()).collect(),
    }
}

#[test]
fn editing_another_line_keeps_a_rules_date() {
    let now = ms("2026-10-21T18:00:00Z");
    let (_, first) = date_lines(
        &[dated_doc(&[GLOB_LINE], "2026-10-15")],
        &LineLedger::default(),
        now,
    );
    let edited = dated_doc(&["- A new line.", GLOB_LINE], "2026-10-21");
    let (dates, _) = date_lines(std::slice::from_ref(&edited), &first, now);
    assert_eq!(dates.date_of(&edited, 1), Some(ms("2026-10-15T12:00:00Z")));
    assert_eq!(dates.date_of(&edited, 0), Some(ms("2026-10-21T12:00:00Z")));
    let asked = Asked::default();
    let rule = check_row(
        &row(CauseId::ZshGlobFlag),
        std::slice::from_ref(&edited),
        &Recording(&asked),
        &dates,
        &Redactor::new(true),
    )
    .expect("rule");
    assert_eq!(
        rule.since_iso.as_deref().map(|s| &s[..10]),
        Some("2026-10-15")
    );
    let days: Vec<String> = asked
        .borrow()
        .last()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|c| c.1)
        .collect();
    assert_eq!(days, ["2026-10-15"]);
}

#[test]
fn the_ledger_stores_hashes_and_drops_gone_lines() {
    let now = ms("2026-10-21T18:00:00Z");
    let secret_line = format!("token {SECRET}");
    let (_, first) = date_lines(
        &[dated_doc(&[GLOB_LINE, &secret_line], "2026-10-15")],
        &LineLedger::default(),
        now,
    );
    assert!(
        first
            .files
            .iter()
            .all(|(p, keys)| !p.contains("sk-ant") && keys.keys().all(|k| !k.contains("sk-ant")))
    );
    let (_, second) = date_lines(&[dated_doc(&[GLOB_LINE], "2026-10-21")], &first, now);
    assert_eq!(
        second
            .files
            .get("/u/.agents/AGENTS.md")
            .map(indexmap::IndexMap::len),
        Some(1)
    );
}
