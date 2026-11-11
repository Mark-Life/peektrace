//! The cause detectors: what each one claims, and the greps, probes and
//! echoes that print the same words without being one.

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
