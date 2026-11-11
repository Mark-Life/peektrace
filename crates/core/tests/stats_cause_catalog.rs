//! The non-glob cause detectors (persisted output, file edits, scratch
//! modules, ugrep), the catalog strings, extraction and the rule probes.

mod common;

use peektrace_core::stats::detectors::causes::{CauseHit, detect_cause, spec};
use peektrace_core::stats::schema::CauseId;

fn bash(cmd: &str, output: &str) -> Option<CauseHit> {
    detect_cause("Bash", cmd, output, &|s: &str| s.to_owned())
}

fn cause(cmd: &str, output: &str) -> Option<CauseId> {
    bash(cmd, output).map(|h| h.cause)
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
