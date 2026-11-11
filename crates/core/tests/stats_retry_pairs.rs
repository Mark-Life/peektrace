//! Retry pairs: which later call counts as the retry of a failed one, and
//! what change it learned.
#![allow(clippy::float_cmp, reason = "counts are whole numbers")]

mod common;

use common::causes::*;
use common::{bash_fact, bash_mut, tool_fact};
use peektrace_core::stats::schema::{CallFact, CauseId, Kind, WaitShape};

macro_rules! facts {
    ($($e:expr),* $(,)?) => { vec![$(In::from($e)),*] };
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
