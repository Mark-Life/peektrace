//! Retry rows: the change a retry pair learned, and what else the row counts.
#![allow(clippy::float_cmp, reason = "counts are whole numbers")]

mod common;

use common::causes::*;
use common::{bash_fact, bash_mut};
use peektrace_core::stats::schema::{CallFact, CauseId, Kind, WaitShape};

macro_rules! facts {
    ($($e:expr),* $(,)?) => { vec![$(In::from($e)),*] };
}

#[test]
fn an_escaped_quote_does_not_split_the_learned_diff() {
    let cmd = "grep -rn \"key\\\": 1\" src --include=*.ts";
    let rows = rows_of(facts![
        with(glob(0), |f| {
            set_cmd(f, cmd);
            f.focus = s(cmd);
        }),
        with(ok(1), |f| set_cmd(
            f,
            "grep -rn \"key\\\": 1\" src --include='*.ts'"
        )),
    ]);
    assert_eq!(
        diffs(&rows, GLOB),
        Some(diff("--include=*.ts", "--include='*.ts'"))
    );
}

fn cnf(cmd: &str, arg: &str, focus: Option<&str>) -> CallFact {
    with(bash_fact(cmd), |f| {
        f.ts = s(&at(0.0));
        f.failed = true;
        f.cause = Some(CauseId::CommandNotFound);
        f.cause_arg = s(arg);
        f.focus = focus.map(str::to_owned);
    })
}

fn retry(cmd: &str) -> CallFact {
    with(bash_fact(cmd), |f| {
        f.seq = 1.0;
        f.ts = s(&at(5.0));
    })
}

const CNF: &str = "cause:command-not-found";

#[test]
fn a_missing_binary_retried_by_the_same_name_learns_no_diff() {
    let rows = rows_of(facts![cnf("jq .x f", "jq", Some(".x f")), retry("jq .x f")]);
    assert_eq!(pairs(&rows, CNF), Some(1.0));
    assert_eq!(diffs(&rows, CNF), Some(vec![]));
}

#[test]
fn context_tokens_count_once_per_model_call() {
    let other = |seq: u32| {
        with(glob(seq), |f| {
            set_cmd(f, "find docs -name *.md");
            f.cause_arg = s("*.md");
            f.focus = s("find docs -name *.md");
        })
    };
    let rows = rows_of(facts![
        glob(0),
        other(1),
        with(ok(2), |f| {
            f.req = s("rX");
            f.ctx_tok = Some(5000.0);
        }),
        with(ok(3), |f| {
            f.req = s("rX");
            f.ctx_tok = Some(5000.0);
            set_cmd(f, "find docs -name '*.md'");
        }),
    ]);
    let r = row(&rows, GLOB).expect("row");
    assert_eq!(
        (r.pairs, r.tokens_measured, r.retry_ctx_tokens),
        (2.0, 2.0, 5000.0)
    );
    let rows = rows_of(facts![glob(0), with(ok(1), |f| f.ctx_tok = None)]);
    let r = row(&rows, GLOB).expect("row");
    assert_eq!(
        (r.pairs, r.tokens_measured, r.retry_ctx_tokens),
        (1.0, 0.0, 0.0)
    );
}

#[test]
fn main_and_nested_calls_are_split() {
    let nested = |f: CallFact| {
        with(f, |f| {
            f.base.sid = "agent-1".into();
            f.base.kind = Kind::Subagent;
        })
    };
    let rows = rows_of(facts![
        glob(0),
        ok(1),
        nested(glob(0)),
        nested(ok(1)),
        nested(ok(2)),
        nested(ok(3)),
    ]);
    let r = row(&rows, GLOB).expect("row");
    assert_eq!(r.hits_by, split(1.0, 1.0));
    assert_eq!(r.pairs_by, split(1.0, 1.0));
    assert_eq!(r.per1k, split(500.0, 250.0));
    assert_eq!(r.sessions, 2.0);
}

#[test]
fn a_missing_binary_may_be_retried_by_any_shell_command() {
    let rows = rows_of(facts![
        err(cnf(
            "jq .name package.json",
            "jq",
            Some(".name package.json")
        )),
        retry("bun .name package.json"),
    ]);
    let r = row(&rows, CNF).expect("row");
    assert_eq!(r.pairs, 1.0);
    assert_eq!(r.diffs, diff("jq", "bun"));
    assert!(r.fix.as_deref().unwrap_or("").contains("install jq"));
}

#[test]
fn a_binary_that_ran_after_its_last_miss_is_installed() {
    let miss = |seq: u32, bin: &str| {
        let n = f64::from(seq);
        err(with(cnf(&format!("{bin} x"), bin, Some("x")), |f| {
            f.seq = n;
            f.ts = s(&at(n * 5.0));
        }))
    };
    let fix_of = |facts: Vec<In>| row(&rows_of(facts), CNF).and_then(|r| r.fix.clone());
    let ran = || {
        with(bash_fact("curl -s u | timeout 5 jq ."), |f| {
            f.seq = 9.0;
            f.ts = s(&at(45.0));
        })
    };
    assert_eq!(
        fix_of(facts![miss(0, "jq"), miss(1, "rsync"), ran()]).as_deref(),
        Some("install rsync")
    );
    assert!(
        fix_of(facts![ran(), miss(10, "jq")])
            .unwrap_or_default()
            .contains("install jq")
    );
    assert!(
        fix_of(facts![miss(0, "jq"), with(ran(), |f| f.err = true)])
            .unwrap_or_default()
            .contains("install jq")
    );
    assert_eq!(fix_of(facts![miss(0, "jq"), ran()]), None);
    let mut t = table_of(
        facts![miss(0, "strings"), miss(1, "rsync"), miss(2, "rsync")],
        None,
    );
    assert_eq!(t.missing_bins(), ["rsync", "strings"]);
    t.mark_installed(&["strings".to_owned()]);
    let rows = finish(&mut t);
    assert_eq!(
        row(&rows, CNF).and_then(|r| r.fix.as_deref()),
        Some("install rsync")
    );
}

#[test]
fn a_missing_binary_with_no_focus_learns_no_diff() {
    let rows = rows_of(facts![
        cnf("cd /p && bc -l x", "bc", None),
        retry("cd /p && bc -l x"),
    ]);
    assert_eq!(pairs(&rows, CNF), Some(1.0));
    assert_eq!(diffs(&rows, CNF), Some(vec![]));
}

#[test]
fn a_missing_binary_learns_the_binary_the_retry_ran() {
    let diffs_of = |cmd: &str| {
        let rows = rows_of(facts![
            cnf("rsync -a src/ dst/", "rsync", Some("rsync -a src/ dst/")),
            retry(cmd),
        ]);
        diffs(&rows, CNF).unwrap_or_default()
    };
    assert_eq!(diffs_of("(cd src/ dst/"), []);
    assert_eq!(diffs_of("(cp -a src/ dst/)"), diff("rsync", "cp"));
    assert_eq!(diffs_of("timeout 30 cp -a src/ dst/"), diff("rsync", "cp"));
    assert_eq!(
        diffs_of("nice -n 10 /bin/cp -a src/ dst/"),
        diff("rsync", "cp")
    );
    assert_eq!(diffs_of("/usr/bin/rsync -a src/ dst/"), []);
}

#[test]
fn a_culprit_missing_from_the_clipped_command_learns_no_diff() {
    let rows = rows_of(facts![
        with(
            bash_fact("grep -rn foo src --exclude-dir=node_modules"),
            |f| {
                f.ts = s(&at(0.0));
                f.failed = true;
                f.cause = Some(CauseId::ZshGlobPath);
                f.cause_arg = s("*.ts");
            }
        ),
        retry("grep -rn foo src --exclude-dir=node_module"),
    ]);
    assert_eq!(pairs(&rows, "cause:zsh-glob-path"), Some(1.0));
    assert_eq!(diffs(&rows, "cause:zsh-glob-path"), Some(vec![]));
}

#[test]
fn a_blocked_sleep_learns_the_wait_that_replaced_it() {
    let blocked = || {
        with(bash_fact("sleep 200 && cat out.log"), |f| {
            f.ts = s(&at(0.0));
            f.failed = true;
            f.cause = Some(CauseId::SleepBlocked);
            f.cause_arg = s("sleep 200");
            f.focus = s("cat out.log");
        })
    };
    let diff_of = |r: CallFact| {
        diffs(&rows_of(facts![blocked(), r]), "cause:sleep-blocked").unwrap_or_default()
    };
    assert_eq!(
        diff_of(with(
            retry("until grep -q DONE out.log; do sleep 2; done; cat out.log"),
            |f| bash_mut(f).wait = Some(WaitShape::LoopPoll)
        )),
        diff("sleep 200", "poll loop")
    );
    assert_eq!(
        diff_of(retry("cat out.log")),
        diff("sleep 200", "(no wait)")
    );
    assert_eq!(
        diff_of(retry(
            "timeout 100 bash -c 'until grep -q \"D START\" out.log; do sleep 3; done'; cat out.log"
        )),
        diff("sleep 200", "poll loop")
    );
}

#[test]
fn with_no_culprit_a_moved_word_is_never_the_change() {
    let rows = rows_of(facts![
        with(
            bash_at(
                0,
                0.0,
                "cd /p && rm -rf $D/out && bun run build --filter=inspector --force"
            ),
            |f| {
                f.failed = true;
                f.cause = Some(CauseId::RmGuard);
            }
        ),
        bash_at(
            1,
            5.0,
            "rm -rf /p/out && cd /p && bun run build --filter=inspector --force"
        ),
    ]);
    assert_eq!(
        diffs(&rows, "cause:rm-guard"),
        Some(diff("$D/out", "/p/out"))
    );
}

#[test]
fn a_persisted_output_read_back_is_a_wasted_call() {
    let rows = rows_of(facts![
        with(bash_at(0, 0.0, "git diff main"), |f| {
            f.req = None;
            bash_mut(f).family = "git diff".into();
            f.cause = Some(CauseId::OutputTooLarge);
            f.cause_arg = s("b1x.txt");
        }),
        with(tool_at(1, 5.0, "Read"), |f| f.arg =
            s("/h/tool-results/b1x.txt")),
    ]);
    let r = row(&rows, "cause:output-too-large").expect("row");
    assert!(!r.failing);
    assert_eq!((r.hits, r.flagged, r.pairs), (1.0, 0.0, 1.0));
    assert_eq!(r.details, [("git diff".to_owned(), 1.0)]);
    assert!(r.fix.as_deref().unwrap_or("").contains("git diff --stat"));
}

#[test]
fn a_structured_output_resend_pairs_and_names_the_field() {
    let so = |seq: u32| {
        with(tool_at(seq, f64::from(seq), "StructuredOutput"), |f| {
            f.req = Some(format!("r{seq}"));
        })
    };
    let rows = rows_of(facts![
        with(err(so(0)), |f| {
            f.cause = Some(CauseId::StructuredOutputSchema);
            f.cause_arg = s("severity");
        }),
        so(1),
    ]);
    let r = row(&rows, "cause:structured-output-schema").expect("row");
    assert_eq!(r.pairs, 1.0);
    assert_eq!(r.kind.as_str(), "workflow");
    assert!(r.fix.as_deref().unwrap_or("").contains("'severity'"));
}
