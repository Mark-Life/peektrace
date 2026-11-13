//! The owned root and group help pages, the bare-command banner and
//! `--completions`. Leaf pages stay with clap. Which page a command line asks
//! for is read from the raw argv, before clap parses it, as the TS CLI does.

use crate::args::Shell;
use crate::reorder::GLOBAL_VALUES;

/// One-line product description, shared by the banner and the root page.
pub const DESCRIPTION: &str = "Local, loopback-only inspector for Claude Code, Codex, Pi and OpenCode sessions: context-budget forensics, stats and Claude memory.";

/// Every runnable command, in display order.
const COMMAND_ROWS: [(&str, &str); 17] = [
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
    (
        "upgrade",
        "Upgrade the installed binary to the latest release",
    ),
];

const GLOBAL_FLAGS: [(&str, &str); 10] = [
    ("--json", "Emit raw JSON instead of rendered tables"),
    ("--read-only", "Refuse any mutating command (safe mode)"),
    ("--pretty", "Render aligned tables, not tab-separated text"),
    (
        "--remote <url>",
        "Use `peektrace serve` over HTTP (PEEKTRACE_TOKEN)",
    ),
    (
        "--otel",
        "Log Effect spans to stderr (or set PEEKTRACE_OTEL)",
    ),
    (
        "--no-telemetry",
        "Disable local wide-event telemetry (on by default)",
    ),
    ("--log-level <level>", "Set the minimum log level"),
    ("--completions <shell>", "Print a completion script"),
    ("-v, --version", "Print the version"),
    ("-h, --help", "Show help"),
];

const EXAMPLES: [&str; 3] = [
    "peektrace serve",
    "peektrace sessions analyze --latest",
    "peektrace stats --days 7",
];

/// Group commands with an owned page, and their summaries.
pub const GROUPS: [(&str, &str); 2] = [
    ("sessions", "List and analyze sessions"),
    ("memory", "Browse and delete Claude memory entries"),
];

const INDENT: &str = "  ";
const MAX_WIDTH: usize = 80;

/// Printed for a bare `peektrace`.
pub fn banner() -> String {
    format!(
        "Peektrace — {DESCRIPTION}\nTry: peektrace serve | tui | sessions ls | memory ls | stats | --help"
    )
}

fn h1(text: &str) -> String {
    format!("\x1b[0;1m{text}\x1b[0m\n")
}

/// Each line indented and ended with a newline.
fn indented(lines: impl Iterator<Item = impl std::fmt::Display>) -> String {
    lines.fold(String::new(), |mut out, l| {
        use std::fmt::Write as _;
        let _ = writeln!(out, "{INDENT}{l}");
        out
    })
}

fn pad(rows: &[(&str, &str)]) -> String {
    let width = rows.iter().map(|(l, _)| l.len()).max().unwrap_or(0);
    indented(rows.iter().map(|(l, r)| format!("{l:<width$}  {r}")))
}

/// The description word-wrapped so no line passes 80 columns.
fn description_lines() -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in DESCRIPTION.split(' ') {
        if !line.is_empty() && INDENT.len() + line.len() + 1 + word.len() > MAX_WIDTH {
            lines.push(std::mem::take(&mut line));
            line.push_str(word);
        } else {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
    }
    lines.push(line);
    indented(lines.iter())
}

/// The root `--help` page.
pub fn root_page(version: &str) -> String {
    let examples = indented(EXAMPLES.iter());
    [
        h1(&format!("Peektrace {version}")),
        h1("USAGE"),
        format!("{INDENT}peektrace [global flags] <command>\n"),
        h1("DESCRIPTION"),
        description_lines(),
        h1("COMMANDS"),
        pad(&COMMAND_ROWS),
        h1("GLOBAL OPTIONS"),
        pad(&GLOBAL_FLAGS),
        h1("EXAMPLES"),
        examples,
        "Run peektrace <command> --help for its flags.\n".to_owned(),
    ]
    .join("\n")
}

/// The page of a group command, built from the rows under its prefix.
pub fn group_page(group: &str, version: &str) -> String {
    let summary = GROUPS
        .iter()
        .find(|(g, _)| *g == group)
        .map_or("", |(_, s)| s);
    let prefix = format!("{group} ");
    let rows: Vec<(&str, &str)> = COMMAND_ROWS
        .iter()
        .filter_map(|(path, s)| path.strip_prefix(&prefix).map(|leaf| (leaf, *s)))
        .collect();
    [
        h1(&format!("Peektrace {version}")),
        h1("USAGE"),
        format!("{INDENT}peektrace [global flags] {group} <command>\n"),
        h1("DESCRIPTION"),
        format!("{INDENT}{summary}\n"),
        h1("COMMANDS"),
        pad(&rows),
        format!("Run peektrace {group} <command> --help for its flags.\n"),
    ]
    .join("\n")
}

/// What the raw command line asks for before any command runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Early {
    /// `--completions <shell>`; `Err` holds the value that names no shell.
    Completions(Result<Shell, String>),
    RootHelp,
    GroupHelp(&'static str),
}

fn flag_name(arg: &str) -> &str {
    arg.split('=').next().unwrap_or(arg)
}

fn is_help(arg: &str) -> bool {
    matches!(flag_name(arg), "--help" | "-h")
}

/// The positionals of `args`, skipping the values of value flags.
fn positionals(args: &[String]) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if GLOBAL_VALUES.contains(&a) {
            i += 1;
        } else if !a.starts_with('-') {
            out.push((i, a));
        }
        i += 1;
    }
    out
}

fn shell_of(raw: &str) -> Result<Shell, String> {
    match raw {
        "sh" => Ok(Shell::Sh),
        "bash" => Ok(Shell::Bash),
        "fish" => Ok(Shell::Fish),
        "zsh" => Ok(Shell::Zsh),
        other => Err(other.to_owned()),
    }
}

/// Read `argv` (with the program name). `--completions` wins over help.
pub fn early(command_line: &[String]) -> Option<Early> {
    let args = command_line.get(1..).unwrap_or_default();
    if let Some(at) = args.iter().position(|a| flag_name(a) == "--completions") {
        let value = match args[at].split_once('=') {
            Some((_, v)) => v.to_owned(),
            None => args.get(at + 1).cloned().unwrap_or_default(),
        };
        return Some(Early::Completions(shell_of(&value)));
    }
    let help_at = args.iter().position(|a| is_help(a))?;
    let pos = positionals(args);
    match pos.first() {
        None => Some(Early::RootHelp),
        Some((first_at, _)) if *first_at > help_at => Some(Early::RootHelp),
        Some((_, first)) if pos.len() == 1 => GROUPS
            .iter()
            .find(|(g, _)| g == first)
            .map(|(g, _)| Early::GroupHelp(g)),
        Some(_) => None,
    }
}

/// The completion script for `shell`.
pub fn completions(shell: Shell) -> String {
    use clap_complete::Shell as Target;
    let target = match shell {
        Shell::Sh | Shell::Bash => Target::Bash,
        Shell::Fish => Target::Fish,
        Shell::Zsh => Target::Zsh,
    };
    let mut cmd = <crate::args::Cli as clap::CommandFactory>::command();
    let mut out = Vec::new();
    clap_complete::generate(target, &mut cmd, "peektrace", &mut out);
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        std::iter::once("pt")
            .chain(s.split(' ').filter(|w| !w.is_empty()))
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn early_pages() {
        assert_eq!(early(&argv("--help")), Some(Early::RootHelp));
        assert_eq!(early(&argv("-h sessions")), Some(Early::RootHelp));
        assert_eq!(early(&argv("--json --help")), Some(Early::RootHelp));
        assert_eq!(early(&argv("--remote x --help")), Some(Early::RootHelp));
        assert_eq!(
            early(&argv("sessions --help")),
            Some(Early::GroupHelp("sessions"))
        );
        assert_eq!(
            early(&argv("memory --json -h")),
            Some(Early::GroupHelp("memory"))
        );
        assert_eq!(early(&argv("sessions ls --help")), None);
        assert_eq!(early(&argv("stats --help")), None);
        assert_eq!(early(&argv("sessions ls")), None);
        assert_eq!(
            early(&argv("--completions zsh --help")),
            Some(Early::Completions(Ok(Shell::Zsh)))
        );
        assert_eq!(
            early(&argv("--completions=sh")),
            Some(Early::Completions(Ok(Shell::Sh)))
        );
        assert_eq!(
            early(&argv("--completions foo")),
            Some(Early::Completions(Err("foo".to_owned())))
        );
    }

    /// Every global value flag (the list `reorder` checks against clap)
    /// skips its value before the help page is picked.
    #[test]
    fn global_values_skip_their_value() {
        GLOBAL_VALUES
            .iter()
            .filter(|f| **f != "--completions")
            .for_each(|flag| {
                assert_eq!(
                    early(&argv(&format!("{flag} x sessions --help"))),
                    Some(Early::GroupHelp("sessions")),
                    "{flag}"
                );
            });
    }

    #[test]
    fn description_wraps_at_80() {
        assert_eq!(
            description_lines(),
            "  Local, loopback-only inspector for Claude Code, Codex, Pi and OpenCode\n  sessions: context-budget forensics, stats and Claude memory.\n"
        );
    }

    #[test]
    fn group_page_rows() {
        let p = group_page("memory", "0.0.0-dev");
        assert!(p.starts_with("\x1b[0;1mPeektrace 0.0.0-dev\x1b[0m\n\n\x1b[0;1mUSAGE\x1b[0m\n\n  peektrace [global flags] memory <command>\n"));
        assert!(p.contains("\n  ls    List projects with memory, or the entries of one project\n  show  Show the frontmatter and full body of one entry\n  rm    Delete one entry (blocked under --read-only)\n\nRun peektrace memory <command> --help for its flags.\n"));
    }

    #[test]
    fn scripts_name_the_binary() {
        for shell in [Shell::Sh, Shell::Bash, Shell::Fish, Shell::Zsh] {
            assert!(completions(shell).contains("peektrace"));
        }
    }

    fn words(s: &[&str]) -> Vec<String> {
        std::iter::once("pt")
            .chain(s.iter().copied())
            .map(str::to_owned)
            .collect()
    }

    /// TS `argv.test.ts` `isRootHelp`.
    #[test]
    fn root_help_only_without_a_subcommand() {
        let root = |s: &[&str]| early(&words(s)) == Some(Early::RootHelp);
        assert!(root(&["--help"]));
        assert!(root(&["-h"]));
        assert!(root(&["--remote", "http://x:1", "--help"]));
        assert!(root(&["--log-level", "debug", "--help"]));
        assert!(root(&["--json", "-h"]));
        assert!(!root(&["stats", "--help"]));
        assert!(!root(&["sessions", "-h"]));
        assert!(root(&["--help", "sessions"]));
        assert!(!root(&["--completions", "zsh", "--help"]));
        assert!(!root(&["--completions=zsh", "--help"]));
        assert!(root(&["--help=true"]));
        assert!(!root(&[]));
    }

    /// TS `argv.test.ts` `helpGroup`. `--help sessions` names the group in
    /// TS too, but the root page wins there first, as it does here.
    #[test]
    fn group_help_only_for_group_then_help() {
        let group = |s: &[&str]| match early(&words(s)) {
            Some(Early::GroupHelp(g)) => Some(g),
            _ => None,
        };
        assert_eq!(group(&["sessions", "--help"]), Some("sessions"));
        assert_eq!(group(&["--json", "memory", "-h"]), Some("memory"));
        assert_eq!(
            early(&words(&["--help", "sessions"])),
            Some(Early::RootHelp)
        );
        assert_eq!(group(&["sessions", "ls", "--help"]), None);
        assert_eq!(group(&["stats", "--help"]), None);
        assert_eq!(group(&["sessions"]), None);
        assert_eq!(group(&["--help"]), None);
        assert_eq!(group(&["sessions", "--completions", "zsh", "--help"]), None);
    }

    /// TS `help.test.ts` "rows match the real command tree": root commands
    /// plus group subcommands equal the rows plus the group paths.
    #[test]
    fn rows_match_the_real_command_tree() {
        use std::collections::BTreeSet;
        let cmd = <crate::args::Cli as clap::CommandFactory>::command();
        let mut paths = BTreeSet::new();
        for sub in cmd.get_subcommands() {
            let name = sub.get_name();
            paths.insert(name.to_owned());
            if ["sessions", "memory", "stats"].contains(&name) {
                for leaf in sub.get_subcommands() {
                    paths.insert(format!("{name} {}", leaf.get_name()));
                }
            }
        }
        let want: BTreeSet<String> = COMMAND_ROWS
            .iter()
            .map(|(p, _)| (*p).to_owned())
            .chain(GROUPS.iter().map(|(g, _)| (*g).to_owned()))
            .collect();
        assert_eq!(paths, want);
    }

    /// TS `help.test.ts` "every real root option appears on the page": the
    /// root flags clap knows, minus those the TS lib adds itself, are the
    /// page's own rows, and each shows on the page.
    #[test]
    fn every_real_root_option_appears_on_the_page() {
        use std::collections::BTreeSet;
        const LIB_FLAGS: [&str; 7] = [
            "--completions",
            "--log-level",
            "-h",
            "--help",
            "--wizard",
            "--version",
            "--telemetry",
        ];
        let cmd = <crate::args::Cli as clap::CommandFactory>::command();
        let real: BTreeSet<String> = cmd
            .get_arguments()
            .filter_map(|a| a.get_long().map(|l| format!("--{l}")))
            .filter(|n| !LIB_FLAGS.contains(&n.as_str()))
            .collect();
        assert!(!real.is_empty());
        let rows: BTreeSet<String> = GLOBAL_FLAGS
            .iter()
            .filter_map(|(usage, _)| usage.split(' ').next())
            .map(str::to_owned)
            .filter(|n| !LIB_FLAGS.contains(&n.as_str()) && n != "-v," && n != "-h,")
            .collect();
        assert_eq!(real, rows);
        let page = root_page("0.0.0-dev");
        for name in &real {
            assert!(page.contains(name.as_str()), "{name}");
        }
        for (usage, _) in GLOBAL_FLAGS {
            assert!(page.contains(usage), "{usage}");
        }
    }
}
