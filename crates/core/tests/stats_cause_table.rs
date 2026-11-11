//! Retry pairs and the cause table, over synthetic facts.
#![allow(clippy::float_cmp, reason = "counts are whole numbers")]

mod common;

use common::{bash_fact, bash_mut, ctx_of, session_fact, tool_fact, window};
use peektrace_core::Agent;
use peektrace_core::js::time::{Utc, to_iso_string};
use peektrace_core::stats::accumulate::{CorpusSummary, DrillHit, DrillTable, DrillTarget};
use peektrace_core::stats::detectors::cause_row::CauseRow;
use peektrace_core::stats::detectors::cells::call_fix_of;
use peektrace_core::stats::detectors::retries::CauseTable;
use peektrace_core::stats::detectors::rules::{
    CauseCounts, CodexCover, CoverSince, RuleCover, Split,
};
use peektrace_core::stats::redact::Redactor;
use peektrace_core::stats::schema::{CallFact, CauseId, Fact, Kind, SessionFact, WaitShape};

const GLOB: &str = "cause:zsh-glob-flag";
const START_MS: i64 = 1_785_578_400_000;

fn at(sec: f64) -> String {
    #[allow(clippy::cast_possible_truncation, reason = "whole ms")]
    let ms = (sec * 1000.0) as i64;
    to_iso_string(START_MS + ms)
}

fn with(mut f: CallFact, edit: impl FnOnce(&mut CallFact)) -> CallFact {
    edit(&mut f);
    f
}

#[allow(clippy::unnecessary_wraps, reason = "fills Option fields")]
fn s(v: &str) -> Option<String> {
    Some(v.to_owned())
}

fn glob(seq: u32) -> CallFact {
    let n = f64::from(seq);
    with(bash_fact("grep -rn foo --include=*.ts src | head"), |f| {
        f.seq = n;
        f.ts = s(&at(n * 5.0));
        f.req = Some(format!("r{seq}"));
        f.ctx_tok = Some(1000.0 * (n + 1.0));
        f.failed = true;
        f.cause = Some(CauseId::ZshGlobFlag);
        f.cause_arg = s("--include=*.ts");
        f.focus = s("grep -rn foo --include=*.ts src");
    })
}

fn ok(seq: u32) -> CallFact {
    let n = f64::from(seq);
    with(bash_fact("grep -rn foo --include='*.ts' src | head"), |f| {
        f.seq = n;
        f.ts = s(&at(n * 5.0));
        f.req = Some(format!("r{seq}"));
        f.ctx_tok = Some(1000.0 * (n + 1.0));
    })
}

/// A bash fact at `at(sec)` with request `r<seq>`.
fn bash_at(seq: u32, sec: f64, cmd: &str) -> CallFact {
    with(bash_fact(cmd), |f| {
        f.seq = f64::from(seq);
        f.ts = s(&at(sec));
        f.req = Some(format!("r{seq}"));
    })
}

fn tool_at(seq: u32, sec: f64, tool: &str) -> CallFact {
    with(tool_fact(), |f| {
        f.seq = f64::from(seq);
        f.ts = s(&at(sec));
        tool.clone_into(&mut f.tool);
    })
}

fn set_cmd(f: &mut CallFact, cmd: &str) {
    let b = bash_mut(f);
    cmd.clone_into(&mut b.cmd);
    b.ran = peektrace_core::stats::shell_lex::ran_bins(cmd);
}

fn err(mut f: CallFact) -> CallFact {
    f.err = true;
    f.failed = true;
    f
}

fn session(sid: &str, edit: impl FnOnce(&mut SessionFact)) -> Fact {
    let mut f = session_fact();
    sid.clone_into(&mut f.base.sid);
    edit(&mut f);
    Fact::Session(f)
}

enum In {
    F(Fact),
}

impl From<CallFact> for In {
    fn from(f: CallFact) -> Self {
        Self::F(Fact::Call(f))
    }
}

impl From<Fact> for In {
    fn from(f: Fact) -> Self {
        Self::F(f)
    }
}

macro_rules! facts {
    ($($e:expr),* $(,)?) => { vec![$(In::from($e)),*] };
}

fn table_of(facts: Vec<In>, drill: Option<&str>) -> CauseTable<'static> {
    let target = drill.map(|k| DrillTarget {
        table: DrillTable::Cause,
        key: k.to_owned(),
    });
    let mut t = CauseTable::new(target.as_ref(), &Utc);
    for In::F(f) in facts {
        t.add(&f);
    }
    t
}

fn finish(t: &mut CauseTable<'_>) -> Vec<CauseRow> {
    let (corpus, window, redact) = (CorpusSummary::default(), window(), Redactor::new(true));
    t.finish(&mut ctx_of(&corpus, &window, &redact))
}

fn rows_of(facts: Vec<In>) -> Vec<CauseRow> {
    finish(&mut table_of(facts, None))
}

fn row<'a>(rows: &'a [CauseRow], key: &str) -> Option<&'a CauseRow> {
    rows.iter().find(|r| r.key == key)
}

fn pairs(rows: &[CauseRow], key: &str) -> Option<f64> {
    row(rows, key).map(|r| r.pairs)
}

fn diffs(rows: &[CauseRow], key: &str) -> Option<Vec<(String, String, f64)>> {
    row(rows, key).map(|r| r.diffs.clone())
}

fn diff(from: &str, to: &str) -> Vec<(String, String, f64)> {
    vec![(from.to_owned(), to.to_owned(), 1.0)]
}

fn split(main: f64, nested: f64) -> Split {
    Split { main, nested }
}

#[test]
fn glob_abort_then_quoted_command_is_one_pair() {
    let rows = rows_of(facts![glob(0), ok(1)]);
    let r = row(&rows, GLOB).expect("row");
    assert_eq!((r.hits, r.flagged, r.pairs), (1.0, 0.0, 1.0));
    assert_eq!((r.retry_sessions, r.retry_sec), (1.0, 5.0));
    assert_eq!(r.retry_ctx_tokens, 2000.0);
    assert_eq!(r.diffs, diff("--include=*.ts", "--include='*.ts'"));
    assert_eq!(r.details, [("*.ts".to_owned(), 1.0)]);
    assert_eq!(r.pair_samples[0].retry.seq, 1.0);
}

#[test]
fn parallel_siblings_never_pair() {
    let rows = rows_of(facts![glob(0), with(ok(1), |f| f.req = s("r0"))]);
    assert_eq!(pairs(&rows, GLOB), Some(0.0));
    let rows = rows_of(facts![
        with(glob(0), |f| f.req = None),
        with(ok(1), |f| {
            f.req = None;
            f.ts = s(&at(0.0));
        }),
    ]);
    assert_eq!(pairs(&rows, GLOB), Some(0.0));
}

#[test]
fn a_sidechain_call_never_retries_a_main_call() {
    let rows = rows_of(facts![glob(0), with(ok(1), |f| f.side = true)]);
    assert_eq!(pairs(&rows, GLOB), Some(0.0));
}

#[test]
fn fail_fail_pass_is_one_pair_over_two_hits() {
    let rows = rows_of(facts![glob(0), glob(1), ok(2)]);
    let r = row(&rows, GLOB).expect("row");
    assert_eq!((r.hits, r.pairs), (2.0, 1.0));
    assert_eq!(r.pair_samples[0].failed.seq, 1.0);
}

#[test]
fn one_success_is_claimed_once_by_the_latest_seed() {
    let e = |seq: u32, cmd: &str| err(bash_at(seq, f64::from(seq), cmd));
    let rows = rows_of(facts![
        e(0, "aaaa bbbb"),
        e(1, "bbbb cccc"),
        bash_at(2, 2.0, "aaaa bbbb cccc"),
    ]);
    let r = row(&rows, "cause:unclassified:Bash").expect("row");
    assert_eq!((r.hits, r.pairs), (2.0, 1.0));
    assert_eq!(r.pair_samples[0].failed.seq, 1.0);
}

#[test]
fn parallel_failed_siblings_each_pair_with_one_fix() {
    let mut all = Vec::new();
    for seq in 0..3 {
        all.push(In::from(with(glob(seq), |f| {
            f.req = s("r1");
            f.ts = s(&at(0.0));
        })));
    }
    for seq in 3..6 {
        all.push(In::from(with(ok(seq), |f| {
            f.req = s("r2");
            f.ts = s(&at(4.0));
        })));
    }
    let rows = rows_of(all);
    let r = row(&rows, GLOB).expect("row");
    assert_eq!((r.hits, r.pairs), (3.0, 3.0));
}

#[test]
fn a_clipped_commands_cut_word_is_never_the_change() {
    let words: Vec<String> = (0..40).map(|i| format!("w{i}")).collect();
    let full = format!(
        "git add {} && git commit -m \"docs: plan the providers beside Codex\"",
        words.join(" ")
    );
    let cut = format!("{}…", &full[..full.len() - 8]);
    let rows = rows_of(facts![
        err(bash_at(0, 0.0, &cut)),
        bash_at(1, 5.0, &format!("{full} && git push")),
    ]);
    assert_eq!(pairs(&rows, "cause:unclassified:Bash"), Some(1.0));
    assert_eq!(diffs(&rows, "cause:unclassified:Bash"), Some(vec![]));
}

#[test]
fn a_seed_keeps_its_best_retry() {
    let rows = rows_of(facts![
        glob(0),
        with(ok(1), |f| set_cmd(f, "grep -rn fo --include='*.ts' src/x")),
        ok(2),
    ]);
    assert_eq!(
        row(&rows, GLOB).expect("row").pair_samples[0].retry.seq,
        2.0
    );
}

#[test]
fn tsc_edit_tsc_is_work_not_a_retry() {
    let rows = rows_of(facts![
        with(err(bash_fact("bun run typecheck")), |f| f.fail_reason =
            s("tsc")),
        with(tool_fact(), |f| {
            f.seq = 1.0;
            f.tool = "Edit".into();
            f.arg = s("/p/a.ts");
        }),
        with(bash_fact("bun run typecheck"), |f| f.seq = 2.0),
    ]);
    assert_eq!(rows, []);
}

#[test]
fn an_unclassified_failure_with_an_edit_between_is_not_paired() {
    let fail = || err(with(bash_at(0, 0.0, "make thing"), |f| f.req = None));
    let pass = || with(bash_at(2, 2.0, "make thing"), |f| f.req = None);
    let edited = rows_of(facts![fail(), tool_at(1, 1.0, "Write"), pass()]);
    assert!(row(&edited, "cause:unclassified:Bash").is_none());
    let read = rows_of(facts![fail(), tool_at(1, 1.0, "Read"), pass()]);
    assert_eq!(pairs(&read, "cause:unclassified:Bash"), Some(1.0));
}

#[test]
fn a_rejected_call_is_never_a_hit() {
    for sig in [
        "The user doesn't want to proceed with this tool use.",
        "The user doesn't want to take this action right now.",
    ] {
        let rejected = err(with(tool_fact(), |f| {
            f.tool = "Edit".into();
            f.sig = s(sig);
        }));
        let next = with(tool_fact(), |f| {
            f.seq = 1.0;
            f.tool = "Edit".into();
        });
        assert_eq!(rows_of(facts![rejected, next]), []);
    }
}

#[test]
fn every_agents_file_edit_stops_an_unclassified_pair() {
    let lower = |f: CallFact| with(f, |f| f.req = None);
    let fail = || {
        err(with(lower(bash_at(0, 0.0, "make thing")), |f| {
            f.tool = "bash".into();
        }))
    };
    let pass = || {
        with(lower(bash_at(2, 2.0, "make thing")), |f| {
            f.tool = "bash".into();
        })
    };
    let key = "cause:unclassified:bash";
    let rows = rows_of(facts![fail(), tool_at(1, 1.0, "read"), pass()]);
    assert_eq!(pairs(&rows, key), Some(1.0));
    let edits = [
        tool_at(1, 1.0, "edit"),
        tool_at(1, 1.0, "write"),
        tool_at(1, 1.0, "apply_patch"),
        with(lower(bash_at(1, 1.0, "true")), |f| {
            f.tool = "bash".into();
            bash_mut(f).bin = "apply_patch".into();
        }),
    ];
    for edit in edits {
        let rows = rows_of(facts![fail(), edit, pass()]);
        assert!(row(&rows, key).is_none());
    }
}

#[test]
fn a_shell_write_between_failure_and_rerun_stops_the_pair() {
    let cmd = "cd /r && bun run check && git commit -m fix";
    let pairs_with = |line: &str| {
        let rows = rows_of(facts![
            err(with(bash_at(0, 0.0, cmd), |f| f.req = None)),
            bash_at(1, 1.0, line),
            with(bash_at(2, 2.0, cmd), |f| f.req = None),
        ]);
        pairs(&rows, "cause:unclassified:Bash").unwrap_or(0.0)
    };
    for edit in [
        "sed -i 's/a/b/' /r/a.ts",
        "perl -pi -e 's/a/b/' /r/a.ts",
        "python3 - <<'EOF'\np='/r/a.ts'\nopen(p,'w').write('x')\nEOF",
        "cat > /r/a.ts <<'EOF'\nx\nEOF",
        "echo x | tee /r/a.ts",
    ] {
        assert_eq!(pairs_with(edit), 0.0, "{edit}");
    }
    for read in [
        "sed -n '1,5p' /r/a.ts",
        "cat /r/a.ts",
        "git status > /dev/null",
    ] {
        assert_eq!(pairs_with(read), 1.0, "{read}");
    }
}

#[test]
fn a_failed_call_with_no_text_pairs_with_nothing() {
    let argless = |seq: u32, tool: &str| {
        with(tool_at(seq, f64::from(seq), tool), |f| {
            f.req = Some(format!("r{seq}"));
        })
    };
    for tool in ["exec_command", "mcp__foo__bar"] {
        let rows = rows_of(facts![
            with(err(argless(0, tool)), |f| f.sig = s("Exit code 1")),
            argless(1, tool),
        ]);
        assert!(row(&rows, &format!("cause:unclassified:{tool}")).is_none());
    }
    let rows = rows_of(facts![
        with(err(argless(0, "StructuredOutput")), |f| {
            f.cause = Some(CauseId::StructuredOutputSchema);
        }),
        argless(1, "StructuredOutput"),
    ]);
    assert_eq!(pairs(&rows, "cause:structured-output-schema"), Some(1.0));
}

#[test]
fn a_cause_seeded_pair_ignores_an_edit_between() {
    let rows = rows_of(facts![glob(0), tool_at(1, 5.0, "Edit"), ok(2)]);
    assert_eq!(pairs(&rows, GLOB), Some(1.0));
}

#[test]
fn a_retry_more_than_ten_minutes_later_is_not_a_retry() {
    let late = |sec: f64| rows_of(facts![glob(0), with(ok(1), |f| f.ts = s(&at(sec)))]);
    assert_eq!(pairs(&late(601.0), GLOB), Some(0.0));
    assert_eq!(pairs(&late(600.0), GLOB), Some(1.0));
}

#[test]
fn only_the_next_three_calls_are_looked_at() {
    let filler = |seq: u32| {
        with(tool_at(seq, f64::from(seq) * 5.0, "Read"), |f| {
            f.req = Some(format!("r{seq}"));
        })
    };
    let rows = rows_of(facts![glob(0), filler(1), filler(2), ok(3)]);
    assert_eq!(pairs(&rows, GLOB), Some(1.0));
    let rows = rows_of(facts![glob(0), filler(1), filler(2), filler(3), ok(4)]);
    assert_eq!(pairs(&rows, GLOB), Some(0.0));
}

#[test]
fn siblings_of_the_failed_call_do_not_use_up_the_window() {
    let sibling = |seq: u32| with(tool_at(seq, 0.0, "Read"), |f| f.req = s("r0"));
    let rows = rows_of(facts![glob(0), sibling(1), sibling(2), sibling(3), ok(4)]);
    assert_eq!(pairs(&rows, GLOB), Some(1.0));
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

#[test]
fn a_new_session_drops_the_open_seeds() {
    let rows = rows_of(facts![glob(0), with(ok(1), |f| f.base.sid = "s2".into())]);
    assert_eq!(pairs(&rows, GLOB), Some(0.0));
}

#[test]
fn an_unclassified_error_with_no_retry_is_not_a_row() {
    assert_eq!(rows_of(facts![err(bash_fact("x"))]), []);
}

#[test]
fn fd_and_nvm_fixes() {
    let fd = call_fix_of(CauseId::CommandNotFound, Some("fd")).unwrap_or_default();
    assert!(fd.contains("mkdir -p ~/.local/bin && ln -s $(command -v fdfind) ~/.local/bin/fd"));
    assert!(fd.contains("put ~/.local/bin on PATH in ~/.zshenv"));
    let nvm = call_fix_of(CauseId::CommandNotFound, Some("nvm")).unwrap_or_default();
    assert!(nvm.contains("source $NVM_DIR/nvm.sh"));
    assert!(!nvm.contains("put nvm on PATH"));
}

#[test]
fn rows_rank_by_retry_sessions_then_sessions_then_pairs() {
    let rows = rows_of(facts![
        with(err(bash_at(0, 0.0, "pkill -f srv; run")), |f| {
            f.cause = Some(CauseId::PkillSelfKill);
        }),
        glob(1),
        ok(2),
    ]);
    let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, [GLOB, "cause:pkill-self-kill"]);
}

fn cover(agent: Agent, dir: Option<&str>) -> RuleCover {
    RuleCover {
        agent,
        codex: None,
        dir: dir.map(str::to_owned),
        source: None,
    }
}

fn hits_since(t: &CauseTable<'_>, cover: &RuleCover, after_day: &str) -> (Split, Split) {
    t.since(
        GLOB,
        &[CoverSince {
            after_day: after_day.to_owned(),
            cover,
        }],
    )
}

#[test]
fn the_dirs_a_rule_check_reads_are_the_hits_cwds() {
    let t = table_of(
        facts![
            session("s1", |f| f.cwd = s("/work/b")),
            session("s0", |f| f.cwd = s("/work/a")),
            glob(0),
        ],
        None,
    );
    assert_eq!(t.project_dirs(), ["/work/b"]);
}

#[test]
fn an_agent_stored_under_one_project_reads_its_repo() {
    let sub = |f: CallFact| {
        with(f, |f| {
            f.base.sid = "agent".into();
            f.base.kind = Kind::Subagent;
        })
    };
    let mut t = table_of(
        facts![
            session("lead", |f| f.cwd = s("/work/lead")),
            with(glob(0), |f| f.base.sid = "lead".into()),
            session("agent", |f| {
                f.base.kind = Kind::Subagent;
                f.cwd = s("/work/other");
            }),
            sub(glob(1)),
        ],
        None,
    );
    assert_eq!(t.project_dirs(), ["/work/lead", "/work/other"]);
    finish(&mut t);
    let (hits, _) = hits_since(&t, &cover(Agent::Claude, Some("/work/other")), "");
    assert_eq!(hits, split(0.0, 1.0));
}

#[test]
fn a_call_without_its_session_row_falls_back_to_its_project_dir() {
    let t = table_of(
        facts![
            session("s0", |f| f.cwd = s("/work/a")),
            with(glob(0), |f| f.base.sid = "gone".into()),
        ],
        None,
    );
    assert_eq!(t.project_dirs(), ["/work/a"]);
}

#[test]
fn hits_since_a_day_count_whole_days_after_it() {
    let mut t = table_of(
        facts![
            with(glob(0), |f| f.ts = s("2026-08-01T10:00:00.000Z")),
            with(glob(1), |f| {
                f.ts = s("2026-08-02T10:00:00.000Z");
                f.base.sid = "s2".into();
            }),
            with(ok(2), |f| {
                f.ts = s("2026-08-02T11:00:00.000Z");
                f.base.sid = "s3".into();
            }),
        ],
        None,
    );
    finish(&mut t);
    let (hits, calls) = hits_since(&t, &cover(Agent::Claude, None), "2026-08-01");
    assert_eq!(hits, split(1.0, 0.0));
    assert_eq!(calls, split(2.0, 0.0));
}

#[test]
fn a_file_counts_only_the_calls_it_reaches() {
    let at_project = |sid: &str, project: &str, agent: Agent, cwd: &str, seq: u32| {
        let (sid, project, cwd) = (sid.to_owned(), project.to_owned(), cwd.to_owned());
        facts![
            session(&sid, |f| {
                f.base.project.clone_from(&project);
                f.base.agent = agent;
                f.cwd = Some(cwd);
            }),
            with(glob(seq), |f| {
                f.base.sid = sid;
                f.base.project = project;
                f.base.agent = agent;
            }),
        ]
    };
    let mut all = at_project("a", "pa", Agent::Claude, "/work/a/sub", 0);
    all.extend(at_project("b", "pb", Agent::Claude, "/work/b", 1));
    all.extend(at_project("c", "pc", Agent::Codex, "/x", 2));
    let mut t = table_of(all, None);
    finish(&mut t);
    let hits = |agent: Agent, dir: Option<&str>| hits_since(&t, &cover(agent, dir), "").0.main;
    assert_eq!(hits(Agent::Claude, None), 2.0);
    assert_eq!(hits(Agent::Codex, None), 1.0);
    assert_eq!(hits(Agent::Claude, Some("/work/a")), 1.0);
    assert_eq!(hits(Agent::Claude, Some("/work")), 2.0);
    assert_eq!(hits(Agent::Claude, Some("/work/ab")), 0.0);
    assert_eq!(hits(Agent::Codex, Some("/work")), 0.0);
    assert_eq!(hits(Agent::Codex, Some("/x")), 1.0);
}

#[test]
fn a_codex_project_file_reaches_its_repo_but_a_nested_ones() {
    let mut all = Vec::new();
    for (sid, cwd, seq) in [
        ("a", "/r", 0),
        ("b", "/r/pkg/deep", 1),
        ("c", "/r/vendor/inner/src", 2),
    ] {
        all.push(
            session(sid, |f| {
                f.base.project = sid.into();
                f.base.agent = Agent::Codex;
                f.cwd = s(cwd);
            })
            .into(),
        );
        all.push(
            with(glob(seq), |f| {
                f.base.sid = sid.into();
                f.base.project = sid.into();
                f.base.agent = Agent::Codex;
            })
            .into(),
        );
    }
    let mut t = table_of(all, None);
    finish(&mut t);
    let hits = |exact: bool, stops: &[&str]| {
        let c = RuleCover {
            codex: Some(CodexCover {
                exact,
                stops: stops.iter().map(|x| (*x).to_owned()).collect(),
            }),
            ..cover(Agent::Codex, Some("/r"))
        };
        hits_since(&t, &c, "").0.main
    };
    assert_eq!(hits(false, &[]), 3.0);
    assert_eq!(hits(false, &["/r/vendor/inner"]), 2.0);
    assert_eq!(hits(true, &[]), 1.0);
}

#[test]
fn a_sourced_global_file_reaches_only_its_sources_sessions() {
    let mut t = table_of(
        facts![
            session("d", |f| f.cwd = s("/w")),
            with(glob(0), |f| f.base.sid = "d".into()),
            session("w", |f| {
                f.cwd = s("/w");
                f.source = s("work");
            }),
            with(glob(1), |f| f.base.sid = "w".into()),
            with(glob(2), |f| f.base.sid = "w".into()),
        ],
        None,
    );
    finish(&mut t);
    let hits = |id: &str, is_default: bool| {
        let c = RuleCover {
            source: Some((id.to_owned(), is_default)),
            ..cover(Agent::Claude, None)
        };
        hits_since(&t, &c, "").0.main
    };
    assert_eq!(hits("work", false), 2.0);
    assert_eq!(hits("claude", true), 1.0);
}

fn drill_of(facts: Vec<In>) -> (Vec<CauseRow>, Vec<DrillHit>) {
    let mut t = table_of(facts, Some(GLOB));
    let rows = finish(&mut t);
    let hits = t.drill(&Redactor::new(true));
    (rows, hits)
}

#[test]
fn drill_returns_pairs_first_with_the_retry_line() {
    let (_, hits) = drill_of(facts![
        glob(0),
        ok(1),
        with(glob(2), |f| f.base.sid = "s2".into())
    ]);
    let got: Vec<(&str, f64)> = hits
        .iter()
        .map(|h| (h.sample.sid.as_str(), h.sample.seq))
        .collect();
    assert_eq!(got, [("s1", 0.0), ("s2", 2.0)]);
    assert!(!hits[0].sample.line.contains(" → "));
    assert_eq!(
        hits[0].pair_line.as_deref(),
        Some(ok(1).bash.expect("bash").cmd.as_str())
    );
    assert_eq!(hits[1].pair_line, None);
}

#[test]
fn links_carry_the_transcripts_own_call_ids() {
    let (rows, hits) = drill_of(facts![
        with(glob(0), |f| f.tool_use_id = s("t0")),
        with(ok(1), |f| f.tool_use_id = s("t1")),
    ]);
    assert_eq!(rows[0].samples[0].tool_use_id.as_deref(), Some("t0"));
    assert_eq!(
        rows[0].pair_samples[0].retry.tool_use_id.as_deref(),
        Some("t1")
    );
    assert_eq!(
        (
            hits[0].sample.tool_use_id.as_deref(),
            hits[0].pair_tool_use_id.as_deref()
        ),
        (Some("t0"), Some("t1"))
    );
}
