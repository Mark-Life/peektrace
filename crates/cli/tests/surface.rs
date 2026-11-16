//! The CLI surface (TS `help.test.ts`, `cli-surface.test.ts`): banner,
//! owned help pages and completion scripts the shells accept.

mod common;

use std::process::{Command, Stdio};

use common::Home;

#[test]
fn bare_command_prints_the_banner_naming_every_agent() {
    let h = Home::new("surface");
    let r = h.run(&[]);
    assert_eq!(r.code, 0);
    for agent in ["Claude Code", "Codex", "Pi", "OpenCode"] {
        assert!(r.out.contains(agent), "{}", r.out);
    }
    assert!(r.out.starts_with("Peektrace — "));
}

#[test]
fn root_help_lists_every_command_once() {
    let h = Home::new("surface");
    let r = h.run(&["--help"]);
    assert_eq!(r.code, 0);
    for row in [
        "serve",
        "sessions ls",
        "sessions search",
        "stats usage",
        "memory rm",
        "statusline",
        "mcp",
        "upgrade",
    ] {
        assert_eq!(
            r.out.matches(&format!("\n  {row}  ")).count(),
            1,
            "{row}: {}",
            r.out
        );
    }
    assert!(
        r.out
            .ends_with("Run peektrace <command> --help for its flags.\n\n")
    );
}

#[test]
fn completions_win_over_help_and_parse_in_each_shell() {
    let h = Home::new("surface");
    for (shell, check) in [
        ("bash", "bash"),
        ("sh", "bash"),
        ("zsh", "zsh"),
        ("fish", "fish"),
    ] {
        let r = h.run(&["--completions", shell, "--help"]);
        assert_eq!(r.code, 0, "{shell}");
        assert!(r.out.contains("peektrace"), "{shell}");
        let path = h.dir().join(format!("completion.{shell}"));
        std::fs::write(&path, &r.out).expect("write");
        let ran = Command::new(check)
            .arg("-n")
            .arg(&path)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output();
        if let Ok(out) = ran {
            assert!(
                out.status.success(),
                "{shell}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
    let bad = h.run(&["--completions", "tcsh"]);
    assert_eq!(bad.code, 1);
    assert!(bad.err.contains("possible values: sh, bash, fish, zsh"));
}

#[test]
fn telemetry_flags_override_each_other() {
    let h = Home::new("surface");
    for args in [
        ["--telemetry", "sessions", "ls"],
        ["--no-telemetry", "sessions", "ls"],
    ] {
        assert_eq!(h.run(&args).code, 0);
    }
    assert_eq!(
        h.run(&["--telemetry", "--no-telemetry", "sessions", "ls"])
            .code,
        0
    );
}

/// The rows of the owned root page (TS `COMMAND_ROWS`).
const COMMAND_ROWS: [(&str, &str); 18] = [
    ("serve", "Start the local web UI"),
    ("tui", "Browse sessions in a terminal UI"),
    ("sessions ls", "List session headers"),
    ("sessions analyze", "Show context forensics for one session"),
    (
        "sessions export",
        "Export one session to HTML, Markdown or JSON",
    ),
    (
        "sessions search",
        "Find sessions by text, redacted by default",
    ),
    (
        "stats",
        "Rank what fails and what costs time across all sessions",
    ),
    ("stats drill", "Show the evidence behind one finding"),
    (
        "stats prompt",
        "Print a prompt to hand the report to an agent",
    ),
    ("stats usage", "Show token and cost usage over time"),
    ("stats refresh", "Rebuild the stats index"),
    (
        "memory ls",
        "List projects with memory, or the entries of one project",
    ),
    (
        "memory show",
        "Show the frontmatter and full body of one entry",
    ),
    ("memory rm", "Delete one entry (blocked under --read-only)"),
    (
        "statusline",
        "Print one-line context health for a status line",
    ),
    (
        "mcp",
        "Serve read-only session and stats tools over stdio MCP",
    ),
    ("doctor", "Write a redacted telemetry bundle for support"),
    (
        "upgrade",
        "Upgrade the installed binary to the latest release",
    ),
];

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("\x1b[") {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 2..];
        let end = tail
            .find(|c: char| !(c.is_ascii_digit() || c == ';'))
            .unwrap_or(tail.len());
        rest = if tail[end..].starts_with('m') {
            &tail[end + 1..]
        } else {
            &rest[at + 1..]
        };
    }
    out.push_str(rest);
    out
}

/// TS `help.test.ts` "peektrace <args> lists every command compactly".
#[test]
fn root_help_lists_every_command_compactly() {
    let h = Home::new("surface");
    for args in [
        &["--help"][..],
        &["-h"],
        &["--json", "--help"],
        &["--help", "sessions"],
    ] {
        let r = h.run(args);
        assert_eq!(r.code, 0, "{args:?}");
        let text = strip_ansi(&r.out);
        assert!(text.contains("Peektrace"), "{args:?}");
        assert!(text.contains("GLOBAL OPTIONS"), "{args:?}");
        for (path, summary) in COMMAND_ROWS {
            assert!(text.contains(path), "{args:?}: {path}");
            assert!(text.contains(summary), "{args:?}: {summary}");
        }
        assert!(text.contains("EXAMPLES"));
        assert!(!text.contains("--wizard"));
        assert!(!text.contains("A true or false value."));
        let lines: Vec<&str> = text.split('\n').collect();
        assert!(lines.len() <= 55, "{args:?}: {} lines", lines.len());
        for line in lines {
            assert!(line.chars().count() <= 80, "{args:?}: {line}");
        }
    }
}

/// TS `help.test.ts` "--completions wins over --help", in both spellings.
#[test]
fn completions_win_over_help_as_a_zsh_script() {
    let h = Home::new("surface");
    for args in [
        &["--completions", "zsh", "--help"][..],
        &["--completions=zsh", "--help"],
    ] {
        let r = h.run(args);
        assert_eq!(r.code, 0, "{args:?}");
        assert!(r.out.contains("#compdef peektrace"), "{args:?}");
    }
}

/// TS `help.test.ts` "--help=true shows the owned page".
#[test]
fn help_equals_true_shows_the_owned_page() {
    let r = Home::new("surface").run(&["--help=true"]);
    assert_eq!(r.code, 0);
    assert!(strip_ansi(&r.out).contains("GLOBAL OPTIONS"));
}

/// TS `help.test.ts` "completions do not call the positive telemetry flag a
/// disable". The `--no-telemetry` row may say "Disable"; `--telemetry` not.
#[test]
fn completions_do_not_call_the_positive_telemetry_flag_a_disable() {
    let r = Home::new("surface").run(&["--completions", "zsh"]);
    assert_eq!(r.code, 0);
    for line in r.out.lines().filter(|l| l.contains("'--telemetry")) {
        assert!(
            !line.contains("Disable local wide-event telemetry"),
            "{line}"
        );
    }
}

/// TS `help.test.ts` "upgrade --help states each default once, hides the
/// wizard". Leaf pages come from clap, so only the shared parts carry over.
#[test]
fn upgrade_help_states_each_default_once_and_hides_the_wizard() {
    let r = Home::new("surface").run(&["upgrade", "--help"]);
    assert_eq!(r.code, 0);
    let text = strip_ansi(&r.out);
    assert!(text.contains("--check") && text.contains("--version"));
    assert!(text.matches("This setting is optional.").count() <= 1);
    assert!(!text.contains("--wizard"));
}

/// TS `help.test.ts` "<group> --help lists its rows within 100 columns".
#[test]
fn group_help_lists_its_rows_within_100_columns() {
    let h = Home::new("surface");
    for group in ["sessions", "memory"] {
        let r = h.run(&[group, "--help"]);
        assert_eq!(r.code, 0, "{group}");
        let text = strip_ansi(&r.out);
        let prefix = format!("{group} ");
        let rows: Vec<_> = COMMAND_ROWS
            .iter()
            .filter_map(|(p, s)| p.strip_prefix(&prefix).map(|leaf| (leaf, *s)))
            .collect();
        assert_ne!(rows, Vec::new(), "{group}: no rows");
        for (leaf, summary) in rows {
            assert!(text.contains(leaf), "{group}: {leaf}");
            assert!(text.contains(summary), "{group}: {summary}");
        }
        for line in text.split('\n') {
            assert!(line.chars().count() <= 100, "{group}: {line}");
        }
    }
}

/// TS `help.test.ts` "--version and -v print the version".
#[test]
fn version_flags_print_the_version() {
    let h = Home::new("surface");
    for flag in ["--version", "-v"] {
        let r = h.run(&[flag]);
        assert_eq!(r.code, 0, "{flag}");
        assert!(!r.out.trim().is_empty(), "{flag}");
    }
}

/// TS `cli-surface.test.ts` "banner names all four agents": the `Try:`
/// line.
#[test]
fn banner_try_line_names_stats() {
    let r = Home::new("surface").run(&[]);
    let try_line = r.out.lines().find(|l| l.starts_with("Try:")).unwrap_or("");
    assert!(try_line.contains("stats"), "{}", r.out);
    assert!(try_line.contains("| tui |"), "{}", r.out);
}

/// TS `cli-surface.test.ts` "--json works before and after the subcommand".
#[test]
fn json_works_before_and_after_the_subcommand() {
    let h = Home::new("surface");
    for args in [["--json", "sessions", "ls"], ["sessions", "ls", "--json"]] {
        let r = h.run(&args);
        assert_eq!(r.code, 0, "{args:?}: {}", r.err);
        let v: serde_json::Value = serde_json::from_str(&r.out).expect("json");
        assert_eq!(v, serde_json::json!([]), "{args:?}");
    }
}
