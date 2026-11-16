//! Retry pairs: which later call counts as the retry of a failed one.
#![allow(clippy::float_cmp, reason = "counts are whole numbers")]

mod common;

use common::causes::*;
use common::{bash_fact, bash_mut, tool_fact};
use peektrace_core::stats::schema::{CallFact, CauseId};

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
fn a_shell_write_before_the_failure_does_not_stop_the_pair() {
    let cmd = "cd /r && bun run check";
    let rows = rows_of(facts![
        bash_at(0, 0.0, "sed -i 's/a/b/' /r/a.ts"),
        err(with(bash_at(1, 1.0, cmd), |f| f.req = None)),
        with(bash_at(2, 2.0, cmd), |f| f.req = None),
    ]);
    assert_eq!(pairs(&rows, "cause:unclassified:Bash"), Some(1.0));
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
fn a_new_session_drops_the_open_seeds() {
    let rows = rows_of(facts![glob(0), with(ok(1), |f| f.base.sid = "s2".into())]);
    assert_eq!(pairs(&rows, GLOB), Some(0.0));
}

#[test]
fn an_unclassified_error_with_no_retry_is_not_a_row() {
    assert_eq!(rows_of(facts![err(bash_fact("x"))]), []);
}
