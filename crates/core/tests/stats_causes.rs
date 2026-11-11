//! The cause detectors: what each one claims, and the greps, probes and
//! echoes that print the same words without being one.

mod common;

use peektrace_core::stats::detectors::causes::{CauseHit, detect_cause, spec};
use peektrace_core::stats::schema::CauseId;

fn bash(cmd: &str, output: &str) -> Option<CauseHit> {
    detect_cause("Bash", cmd, output, &|s: &str| s.to_owned())
}

fn cause(cmd: &str, output: &str) -> Option<CauseId> {
    bash(cmd, output).map(|h| h.cause)
}

fn arg(cmd: &str, output: &str) -> Option<String> {
    bash(cmd, output).and_then(|h| h.arg)
}

#[test]
fn flag_glob_anywhere_in_the_output() {
    let hit = bash(
        "grep -rn foo src --include=*.ts | head; echo done",
        "(eval):1: no matches found: --include=*.ts\ndone",
    );
    let hit = hit.as_ref();
    assert_eq!(hit.map(|h| h.cause), Some(CauseId::ZshGlobFlag));
    assert_eq!(hit.and_then(|h| h.arg.as_deref()), Some("--include=*.ts"));
    assert_eq!(
        hit.and_then(|h| h.focus.as_deref()),
        Some("grep -rn foo src --include=*.ts")
    );
    assert_eq!(
        hit.and_then(|h| h.line.as_deref()),
        Some("(eval):1: no matches found: --include=*.ts")
    );
}

#[test]
fn glob_flag_values_and_path_globs() {
    assert_eq!(
        cause("rg -g *.ts needle", "zsh:1: no matches found: *.ts"),
        Some(CauseId::ZshGlobFlag)
    );
    assert_eq!(
        cause("find . -name *.md", "(eval):1: no matches found: *.md"),
        Some(CauseId::ZshGlobFlag)
    );
    let hit = bash(
        "ls dir/*.x 2>/dev/null",
        "Exit code 1\n(eval):1: no matches found: dir/*.x",
    );
    assert_eq!(hit.as_ref().map(|h| h.cause), Some(CauseId::ZshGlobPath));
    assert_eq!(hit.and_then(|h| h.arg).as_deref(), Some("dir/*.x"));
    assert_eq!(
        cause(
            "grep -rn x . --include=*.{ts,tsx}",
            "(eval):1: no matches found: --include=*.ts"
        ),
        Some(CauseId::ZshGlobFlag)
    );
}

#[test]
fn quoted_or_mid_line_mentions_are_not_globs() {
    assert_eq!(
        cause(
            "grep -h \"no matches found: --include=*.ts\" $TMPDIR/survey/lines.txt",
            "(eval):1: no matches found: --include=*.ts\n(eval):3: no matches found: x*"
        ),
        None
    );
    assert_eq!(
        cause(
            "grep -rn x . --include='*.{ts,tsx}'",
            "(eval):1: no matches found: --include=*.ts"
        ),
        None
    );
    assert_eq!(
        cause(
            "grep -rn 'no matches' ~/.claude/projects",
            "x.jsonl:12:(eval):1: no matches found: --include=*.ts"
        ),
        None
    );
    assert_eq!(
        cause(
            "jq -r .message.content t.jsonl",
            "(eval):1: no matches found: --include=*.ts"
        ),
        None
    );
}

#[test]
fn culprit_after_tilde_and_var_expansion() {
    let tilde = bash(
        "ls ~/work/foo* 2>/dev/null",
        "Exit code 1\n(eval):1: no matches found: /home/u/work/foo*",
    );
    assert_eq!(tilde.as_ref().map(|h| h.cause), Some(CauseId::ZshGlobPath));
    assert_eq!(
        tilde.as_ref().and_then(|h| h.arg.as_deref()),
        Some("~/work/foo*")
    );
    assert_eq!(
        tilde.and_then(|h| h.focus).as_deref(),
        Some("ls ~/work/foo* 2>/dev/null")
    );
    assert_eq!(
        arg(
            "R=/repo; cat $R/apps/*.log",
            "(eval):1: no matches found: /repo/apps/*.log"
        )
        .as_deref(),
        Some("$R/apps/*.log")
    );
    assert_eq!(
        cause(
            "ls ~/other/foo*",
            "(eval):1: no matches found: /home/u/work/foo*"
        ),
        None
    );
}

#[test]
fn substitutions_heredocs_and_escapes() {
    let hit = bash(
        "for s in A B; do echo \"== $s: $(grep -rlw --include=*.ts \"$s\" apps)\"; done",
        "(eval):1: no matches found: --include=*.ts",
    );
    assert_eq!(hit.as_ref().map(|h| h.cause), Some(CauseId::ZshGlobFlag));
    let cmd = "cat > notes.md <<EOF\n# the CLI's stream\nEOF\ngrep -rn x --include=*.ts .";
    assert_eq!(
        cause(cmd, "(eval):4: no matches found: --include=*.ts"),
        Some(CauseId::ZshGlobFlag)
    );
    assert_eq!(
        cause(
            "cat <<'EOF'\nls src/*.md\nEOF",
            "(eval):1: no matches found: src/*.md"
        ),
        None
    );
    let esc = bash(
        "gh api repos/o/r/commits?sha=v2\\&per_page=1",
        "(eval):1: no matches found: repos/o/r/commits?sha=v2&per_page=1",
    );
    assert_eq!(esc.as_ref().map(|h| h.cause), Some(CauseId::ZshGlobPath));
    assert_eq!(
        esc.and_then(|h| h.arg).as_deref(),
        Some("repos/o/r/commits?sha=v2\\&per_page=1")
    );
}

#[test]
fn equals_abort() {
    let hit = bash(
        "ls; echo === tests ===; git status",
        "Exit code 1\nzsh:1: == not found",
    );
    assert_eq!(hit.as_ref().map(|h| h.cause), Some(CauseId::ZshEqualsAbort));
    assert_eq!(hit.and_then(|h| h.arg).as_deref(), Some("==="));
    assert_eq!(
        cause("echo ==foo", "(eval):1: =foo not found"),
        Some(CauseId::ZshEqualsAbort)
    );
    assert_eq!(
        cause("echo === tests ===; false", "Exit code 1\n=== tests ==="),
        None
    );
    assert_eq!(cause("ls; echo \"===\"; git status", ""), None);
    assert_eq!(
        cause("grep -rn 'not found' log", "(eval):1: == not found"),
        None
    );
    assert_eq!(
        cause("pkill -f srv.ts; sleep 1; echo ===; ls", "Exit code 144"),
        Some(CauseId::PkillSelfKill)
    );
    assert_eq!(
        cause(
            "echo === ; jq . f",
            "Exit code 127\n===\nbash: line 1: jq: command not found"
        ),
        Some(CauseId::CommandNotFound)
    );
}

#[test]
fn pkill_self_kill() {
    for cmd in [
        "pkill -f my-server; bun run dev",
        "pgrep -f srv | xargs kill; echo ok",
        "pkill -u $(id -u) -f 'remote-debugging-port=9876'; rm -rf $D",
        "pkill -9 -f \"bun mock.ts\"",
        "for p in $(pgrep -f foo); do kill $p; done",
        "kill -9 $(pgrep -f foo)",
        "pkill -f '[b]un run dev'; bun run dev &",
        "ps aux | grep '[v]ite' | awk '{print $2}' | xargs kill",
        "kill $(ps -eo pid,args | grep vite | awk '{print $1}')",
    ] {
        assert_eq!(
            cause(cmd, "Exit code 144"),
            Some(CauseId::PkillSelfKill),
            "{cmd}"
        );
    }
    for cmd in ["pkill -9 node; ls -f", "ps aux | grep vite", "bun run dev"] {
        assert_eq!(cause(cmd, "Exit code 144"), None, "{cmd}");
    }
    let fix = spec(CauseId::PkillSelfKill).fix.unwrap_or_default();
    assert!(fix.contains("[p]attern") && fix.contains("pkill -x NAME"));
}

#[test]
fn harness_blocks() {
    let hit = bash(
        "sleep 90; tail -5 /tmp/out.log",
        "<tool_use_error>Blocked: sleep 90 followed by: tail -5 /tmp/out.log. To wait for a condition, use Monitor.</tool_use_error>",
    );
    assert_eq!(hit.as_ref().map(|h| h.cause), Some(CauseId::SleepBlocked));
    assert_eq!(
        hit.as_ref().and_then(|h| h.arg.as_deref()),
        Some("sleep 90")
    );
    assert_eq!(
        hit.and_then(|h| h.focus).as_deref(),
        Some("tail -5 /tmp/out.log")
    );
    assert_eq!(
        cause(
            "rm -f $D/*.txt",
            "Dangerous rm operation detected in `rm -f $D/*.txt`. The target is a shell variable expansion"
        ),
        Some(CauseId::RmGuard)
    );
    assert_eq!(
        cause("cat notes.md", "notes\nBlocked: sleep 90 followed by: tail"),
        None
    );
}

#[test]
fn command_not_found() {
    let zsh = bash(
        "cd /p && jq .name package.json",
        "Exit code 127\n(eval):1: command not found: jq",
    );
    assert_eq!(
        zsh.as_ref().map(|h| h.cause),
        Some(CauseId::CommandNotFound)
    );
    assert_eq!(zsh.as_ref().and_then(|h| h.arg.as_deref()), Some("jq"));
    assert_eq!(
        zsh.and_then(|h| h.focus).as_deref(),
        Some(".name package.json")
    );
    assert_eq!(
        arg(
            "strings bin | head",
            "bash: line 1: strings: command not found"
        )
        .as_deref(),
        Some("strings")
    );
    assert_eq!(
        arg(
            "jq . f",
            "Exit code 127\n/bin/bash: line 1: jq: command not found"
        )
        .as_deref(),
        Some("jq")
    );
    assert_eq!(
        bash(
            "cd /p && FOO=1 sudo jq .x f",
            "(eval):1: command not found: jq"
        )
        .and_then(|h| h.focus)
        .as_deref(),
        Some(".x f")
    );
    assert_eq!(
        cause("echo \"command not found\"", "command not found"),
        None
    );
    for probe in [
        "jq --version || echo missing",
        "jq --version 2>/dev/null || echo missing",
    ] {
        assert_eq!(
            cause(probe, "(eval):1: command not found: jq\nmissing"),
            None,
            "{probe}"
        );
    }
    for probe in [
        "pi --version 2>&1",
        "pi --version >&2",
        "pi -v 2>&1 | head",
        "pi --version &>/dev/null || echo no",
    ] {
        assert_eq!(
            cause(probe, "(eval):1: command not found: pi"),
            None,
            "{probe}"
        );
    }
    let cnf = |b: &str| format!("(eval):1: command not found: {b}");
    assert_eq!(
        cause("command -v jq; jq .", &cnf("jq")),
        Some(CauseId::CommandNotFound)
    );
    assert_eq!(
        arg("which psql; psql -h db -c 'select 1'", &cnf("psql")).as_deref(),
        Some("psql")
    );
    assert_eq!(
        arg("aws --version; aws s3 ls s3://b", &cnf("aws")).as_deref(),
        Some("aws")
    );
}

#[test]
fn glob_inside_substitution_keeps_its_arg() {
    let hit = bash(
        "for s in A B; do echo \"== $s: $(grep -rlw --include=*.ts \"$s\" apps)\"; done",
        "(eval):1: no matches found: --include=*.ts",
    );
    assert_eq!(hit.and_then(|h| h.arg).as_deref(), Some("--include=*.ts"));
}

#[test]
fn pkill_alone_and_its_fix_text() {
    assert_eq!(
        cause("pkill -f my-server", "Exit code 144"),
        Some(CauseId::PkillSelfKill)
    );
    let s = spec(CauseId::PkillSelfKill);
    let fix = s.fix.unwrap_or_default();
    assert!(fix.contains("pkill -f '[b]un run dev'; bun run dev & still dies"));
    assert!(
        !s.advice
            .unwrap_or_default()
            .contains("unless the pattern brackets")
    );
}

#[test]
fn cnf_fix_names_both_startup_files() {
    let fix = spec(CauseId::CommandNotFound).fix.unwrap_or_default();
    for want in ["~/.zshenv", "~/.bashrc", "export PATH"] {
        assert!(fix.contains(want), "{want}");
    }
}

#[test]
fn cnf_binary_not_at_command_position() {
    assert_eq!(cause("cat jq.md", "(eval):1: command not found: jq"), None);
}

const PERSISTED: &str = "<persisted-output>\nOutput too large (51.6KB). Full output saved to: /h/.claude/projects/p/s/tool-results/b1x.txt\n\nPreview:";

#[test]
fn persisted_output() {
    let hit = bash("git diff main", PERSISTED);
    assert_eq!(hit.as_ref().map(|h| h.cause), Some(CauseId::OutputTooLarge));
    assert_eq!(hit.and_then(|h| h.arg).as_deref(), Some("b1x.txt"));
    assert!(!spec(CauseId::OutputTooLarge).failing);
    let preview = format!("{PERSISTED}\n(eval):1: no matches found: --include=*.ts\ncommit 1");
    assert_eq!(
        cause("grep -rn x . --include=*.ts; git log", &preview),
        Some(CauseId::ZshGlobFlag)
    );
    assert!(detect_cause("WebFetch", "", PERSISTED, &|s: &str| s.to_owned()).is_none());
    assert_eq!(
        cause(
            "cat log",
            "line\n<persisted-output>\nOutput too large (1KB)."
        ),
        None
    );
}

#[test]
fn file_editing_tools() {
    let tool = |name: &str, out: &str| {
        detect_cause(name, "", out, &|s: &str| s.to_owned()).map(|h| h.cause)
    };
    assert_eq!(
        tool(
            "Write",
            "<tool_use_error>File has not been read yet. Read it first before writing to it.</tool_use_error>"
        ),
        Some(CauseId::ReadBeforeWrite)
    );
    assert_eq!(
        tool(
            "Edit",
            "<tool_use_error>File has been modified since read, either by the user or by a linter.</tool_use_error>"
        ),
        Some(CauseId::StaleRead)
    );
    assert_eq!(
        tool(
            "Edit",
            "<tool_use_error>String to replace not found in file.</tool_use_error>"
        ),
        Some(CauseId::EditNoMatch)
    );
    assert_eq!(
        tool(
            "Write",
            "<tool_use_error>String to replace not found</tool_use_error>"
        ),
        None
    );
    let hit = detect_cause(
        "StructuredOutput",
        "",
        "Output does not match required schema: /findings/0: must have required property 'severity'",
        &|s: &str| s.to_owned(),
    );
    assert_eq!(
        hit.as_ref().map(|h| h.cause),
        Some(CauseId::StructuredOutputSchema)
    );
    assert_eq!(hit.and_then(|h| h.arg).as_deref(), Some("severity"));
}

#[test]
fn scratch_module_resolution() {
    use CauseId::ScratchModuleResolution as S;
    assert_eq!(
        cause(
            "bun /tmp/x/harness.ts",
            "error: Cannot find module 'effect' from '/tmp/x/harness.ts'"
        ),
        Some(S)
    );
    assert_eq!(
        cause(
            "bun src/x.ts",
            "error: Cannot find module 'effect' from '/repo/src/template/x.ts'"
        ),
        None
    );
    assert_eq!(
        cause(
            "node /tmp/x.mjs",
            "Exit code 1\nError [ERR_MODULE_NOT_FOUND]: Cannot find package 'effect' imported from /tmp/x.mjs"
        ),
        Some(S)
    );
    assert_eq!(
        cause(
            "node /tmp/x.cjs",
            "Exit code 1\nError: Cannot find module 'effect'\nRequire stack:\n- /tmp/x.cjs"
        ),
        Some(S)
    );
    assert_eq!(
        cause(
            "npx tsx src/x.ts",
            "Error [ERR_MODULE_NOT_FOUND]: Cannot find package 'effect' imported from /repo/src/x.ts"
        ),
        None
    );
    for path in [
        "/var/tmp/x.ts",
        "/home/u/.cache/agent-tmp/x.ts",
        "/var/folders/ab/cd/T/x.ts",
        "C:\\Users\\u\\AppData\\Local\\Temp\\x.ts",
    ] {
        assert_eq!(
            cause(
                &format!("bun {path}"),
                &format!("error: Cannot find module 'effect' from '{path}'")
            ),
            Some(S),
            "{path}"
        );
    }
    assert_eq!(
        cause(
            "cat /tmp/run/E.log | tail -20",
            "[12:01] child said: error: Cannot find module 'effect' from '/tmp/probe/x.ts'"
        ),
        None
    );
    assert_eq!(
        cause(
            "cat /tmp/run/E.log",
            "error: Cannot find module 'effect' from '/tmp/probe/x.ts'"
        ),
        None
    );
    assert_eq!(
        cause(
            "cd /tmp/probe && bun x.ts; echo done",
            "error: Cannot find module 'effect' from '/tmp/probe/x.ts'\ndone"
        ),
        Some(S)
    );
}

#[test]
fn ugrep_complexity() {
    assert_eq!(
        cause(
            "grep -E '(a|b){0,200}' f",
            "ugrep: error: error at position 11\n(a|b){0,200}\n    exceeds complexity limits"
        ),
        Some(CauseId::UgrepComplexity)
    );
}

#[test]
fn catalog_fix_advice_and_zshenv() {
    use peektrace_core::stats::detectors::causes::CauseKind;
    for &id in CauseId::ALL {
        let s = spec(id);
        assert_eq!(s.fix.is_none(), s.kind == CauseKind::None, "{id:?}");
        assert!(s.probes.is_empty() || s.advice.is_some(), "{id:?}");
    }
    for id in [
        CauseId::ZshGlobFlag,
        CauseId::ZshGlobPath,
        CauseId::ZshEqualsAbort,
    ] {
        let advice = spec(id).advice.unwrap_or_default();
        assert!(advice.contains("~/.zshenv") && !advice.contains("~/.zshrc"));
    }
    let fix = spec(CauseId::ZshGlobPath).fix.unwrap_or_default();
    assert!(!fix.contains("(N)") && !fix.contains("ls -d") && fix.contains("find"));
}

mod extract {
    use super::common::{bash_step, calls, facts_of, facts_of_text, transcript_of};
    use peektrace_core::stats::detectors::similarity::FOCUS_SEP;
    use peektrace_core::stats::schema::CauseId;

    #[test]
    fn exit_zero_chain_hit_fails_the_row() {
        let facts = facts_of(
            &[bash_step(
                "grep -rn foo src --include=*.ts | head -5; echo end",
                "src/a.ts:1:foo\n(eval):1: no matches found: --include=*.ts\nend",
                false,
            )],
            "sess-1",
        );
        let c = calls(&facts)[0];
        assert!(!c.err && c.failed);
        assert_eq!(c.cause, Some(CauseId::ZshGlobFlag));
        assert_eq!(c.cause_arg.as_deref(), Some("--include=*.ts"));
        let focus: Vec<&str> = c.focus.as_deref().unwrap_or("").split(FOCUS_SEP).collect();
        assert_eq!(focus, ["grep -rn foo src --include=*.ts"]);
        assert!(c.sig.as_deref().unwrap_or("").contains("no matches found"));
        assert_eq!(c.benign, None);
    }

    #[test]
    fn secret_in_failing_segment_is_redacted() {
        let key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let facts = facts_of(
            &[bash_step(
                &format!("cd /p && awsx configure set aws_secret_access_key \"{key}\""),
                "Exit code 127\n(eval):1: command not found: awsx",
                true,
            )],
            "sess-1",
        );
        let c = calls(&facts)[0];
        assert_eq!(c.cause, Some(CauseId::CommandNotFound));
        let focus = c.focus.as_deref().expect("focus");
        assert!(!focus.contains(key));
        assert!(!c.bash.as_ref().expect("bash").cmd.contains(key));
    }

    #[test]
    fn unredacted_pass_keeps_the_cause() {
        let text = transcript_of(&[bash_step(
            "jq . f.json",
            "Exit code 127\n(eval):1: command not found: jq",
            true,
        )]);
        let facts = facts_of_text(&text, "sess-1", false, false);
        assert_eq!(calls(&facts)[0].cause, Some(CauseId::CommandNotFound));
    }
}

#[test]
fn rule_probes_need_their_own_words() {
    use peektrace_core::stats::detectors::causes::probes;
    let states = |line: &str| -> Vec<CauseId> {
        CauseId::ALL
            .iter()
            .copied()
            .filter(|&id| {
                probes(id)
                    .iter()
                    .any(|p| p.iter().all(|re| re.is_match(line)))
            })
            .collect()
    };
    assert_eq!(
        states("Always use the Read tool before Edit or Write."),
        [CauseId::ReadBeforeWrite]
    );
    assert_eq!(
        states("Re-read a file after the formatter touched it."),
        [CauseId::StaleRead]
    );
    assert_eq!(
        states("Copy old_string from the exact lines a Read printed."),
        [CauseId::EditNoMatch]
    );
}
