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
