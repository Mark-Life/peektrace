//! Command-line shape. Global flags work before or after the subcommand.

use clap::builder::{OsStringValueParser, TypedValueParser};
use clap::{Args, Parser, Subcommand};

use crate::otel::LogLevel;

/// Local inspector for coding-agent sessions.
#[derive(Debug, Parser)]
#[expect(clippy::struct_excessive_bools, reason = "independent CLI flags")]
#[command(name = "peektrace", disable_version_flag = true)]
pub struct Cli {
    /// Emit raw JSON instead of rendered tables
    #[arg(long, global = true)]
    pub json: bool,

    /// Render padded tables for people instead of tab-separated text
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Refuse any mutating command (safe mode)
    #[arg(long = "read-only", global = true)]
    pub read_only: bool,

    /// Print each finished span to stderr
    #[arg(long, global = true)]
    pub otel: bool,

    /// Minimum level of log lines printed to stderr
    #[arg(long = "log-level", global = true, value_enum)]
    pub log_level: Option<LogLevel>,

    // Accepted for TS flag parity. No telemetry store exists yet, so nothing
    // reads it.
    /// Do not record local telemetry
    #[arg(long = "no-telemetry", global = true)]
    pub no_telemetry: bool,

    /// Print the version
    #[arg(short = 'v', long = "version")]
    pub version: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List and analyze sessions
    Sessions(SessionsArgs),
    /// Cross-session friction: what fails, what costs time, what to fix first
    Stats(StatsArgs),
    /// Browse and manage Claude memory files
    Memory(MemoryArgs),
}

/// `memory` with no subcommand prints its help and exits 0.
#[derive(Debug, Args)]
pub struct MemoryArgs {
    #[command(subcommand)]
    pub command: Option<MemoryCommand>,
}

#[derive(Debug, Subcommand)]
pub enum MemoryCommand {
    /// List projects with memory, or one project's memory files
    Ls(MemoryLsArgs),
    /// Print one memory file: frontmatter fields and body
    Show(MemoryNameArgs),
    /// Delete one memory file and its MEMORY.md line
    Rm(MemoryNameArgs),
}

#[derive(Debug, Args)]
pub struct MemoryLsArgs {
    /// Project slug (default: every project with memory)
    #[arg(allow_hyphen_values = true, value_parser = lossy())]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct MemoryNameArgs {
    /// Project slug
    #[arg(allow_hyphen_values = true, value_parser = lossy())]
    pub project: String,
    /// Memory name (file name without .md)
    #[arg(allow_hyphen_values = true, value_parser = lossy())]
    pub name: String,
}

/// Window, scope and row-cap flags shared by the `stats` commands.
#[derive(Debug, Default, Args)]
pub struct StatsCommon {
    /// History window in days (default 60)
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub days: Option<String>,

    /// Agent to scan: claude|codex|pi|opencode|all
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub agent: Option<String>,

    /// Rows kept per table (default 20)
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub limit: Option<String>,
}

#[derive(Debug, Args)]
pub struct StatsArgs {
    #[command(subcommand)]
    pub command: Option<StatsCommand>,

    #[command(flatten)]
    pub common: StatsCommon,
}

#[derive(Debug, Subcommand)]
pub enum StatsCommand {
    /// The calls behind one row of a stats table
    Drill(StatsDrillArgs),
    /// An investigation prompt for the report or one row of it
    Prompt(StatsPromptArgs),
    /// Re-extract the fact cache without computing a report
    Refresh(StatsRefreshArgs),
    /// Tokens and estimated cost per day, month or 5-hour block
    Usage(StatsUsageArgs),
}

#[derive(Debug, Args)]
pub struct StatsDrillArgs {
    /// Cause-table key to expand into failed → retry pairs
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub cause: Option<String>,
    /// Fail-table key to expand
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub fail: Option<String>,
    /// Time-table job to expand
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub time: Option<String>,
    /// Error-cluster signature to expand
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub cluster: Option<String>,
    /// Waiting-panel key to expand
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub wait: Option<String>,
    #[command(flatten)]
    pub common: StatsCommon,
}

#[derive(Debug, Args)]
pub struct StatsPromptArgs {
    /// Cause id or key, e.g. zsh-glob-flag (default: the whole report)
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub cause: Option<String>,
    /// Fail-table key
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub fail: Option<String>,
    /// Time-table job
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub time: Option<String>,
    #[command(flatten)]
    pub common: StatsCommon,
}

#[derive(Debug, Args)]
pub struct StatsRefreshArgs {
    /// Agent to scan: claude|codex|pi|opencode|all
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub agent: Option<String>,
    /// Ignore cached shards and re-extract every transcript
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct StatsUsageArgs {
    /// Bucket by day, month or 5-hour block
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub by: Option<String>,
    /// Days back from now (default 30 by day, 365 by month, 7 by block)
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub days: Option<String>,
    /// Agent to scan: claude|codex|pi|opencode|all
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub agent: Option<String>,
    /// IANA time zone for day and month keys (default: this machine's)
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub tz: Option<String>,
    /// Ignore cached shards and re-extract every transcript
    #[arg(long)]
    pub force: bool,
}

/// `sessions` with no subcommand prints its help and exits 0.
#[derive(Debug, Args)]
pub struct SessionsArgs {
    #[command(subcommand)]
    pub command: Option<SessionsCommand>,
}

#[derive(Debug, Subcommand)]
pub enum SessionsCommand {
    /// List session headers, newest first
    Ls(LsArgs),
    /// Show context forensics for one session
    Analyze(AnalyzeArgs),
    /// Export one session to HTML, Markdown or JSON
    Export(ExportArgs),
    /// Find sessions whose text holds a phrase
    Search(SearchArgs),
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Text to find (case-insensitive phrase)
    #[arg(required = true, num_args = 1.., value_parser = lossy())]
    pub q: Vec<String>,

    /// Agent: claude|codex|pi|opencode|all (default all)
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub agent: Option<String>,

    /// Filter sessions by project name (Claude slug)
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub project: Option<String>,

    /// Only sessions updated since: ISO date or date-time or 7d, 12h, 2w
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub since: Option<String>,

    /// Max sessions, newest first (0 = all)
    #[arg(long, default_value = "20", allow_hyphen_values = true, value_parser = lossy())]
    pub limit: String,

    /// Keep secrets and home paths in the output (redacted by default)
    #[arg(long = "no-redact")]
    pub no_redact: bool,
}

/// Flag text with bytes that are not UTF-8 turned into U+FFFD, as Node
/// decodes `argv`, instead of rejecting the command.
fn lossy() -> impl TypedValueParser<Value = String> {
    OsStringValueParser::new().map(|s| s.to_string_lossy().into_owned())
}

#[derive(Debug, Args)]
pub struct LsArgs {
    /// Agent: claude|codex|pi|opencode|all (default all)
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub agent: Option<String>,

    /// Filter sessions by project name (Claude slug)
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub project: Option<String>,

    /// Only sessions updated since: ISO date or date-time (2026-01-31 is UTC
    /// midnight; no offset means UTC) or 7d, 12h, 2w
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub since: Option<String>,

    /// Only sessions run in this directory or below (path on this machine)
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub cwd: Option<String>,

    /// Max sessions to list, newest first (0 = all)
    #[arg(long, default_value = "30", allow_hyphen_values = true, value_parser = lossy())]
    pub limit: String,
}

/// Which session a single-session command runs on.
#[derive(Debug, Default, Args)]
pub struct TargetArgs {
    /// Session id, short id, or transcript path
    #[arg(allow_hyphen_values = true, value_parser = lossy())]
    pub id: Option<String>,

    /// Use the newest session of any agent (combine with --cwd to scope it)
    #[arg(long)]
    pub latest: bool,

    /// Only sessions run in this directory or below (path on this machine)
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub cwd: Option<String>,

    /// Parent session id, when <id> is a subagent transcript (agent-*)
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub parent: Option<String>,
}

#[derive(Debug, Args)]
pub struct AnalyzeArgs {
    #[command(flatten)]
    pub target: TargetArgs,
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Output format: html|md|json (default: from --json or the --out
    /// extension, else md)
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub format: Option<String>,

    /// Write to this file instead of stdout (`-` is stdout)
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub out: Option<String>,

    /// Keep secrets and home paths in the output (redacted by default)
    #[arg(long = "no-redact")]
    pub no_redact: bool,
}

/// Flags of `sessions analyze` and `sessions export` that take a value.
const VALUE_FLAGS: [&str; 5] = ["--cwd", "--parent", "--format", "--out", "--log-level"];

/// The flag `@effect/cli` reports as missing its value: a value flag that
/// ends the command line of a single-session command given no id. It reads
/// the flag as the id and prints `Missing value for <flag>.`
pub fn missing_value(argv: &[String]) -> Option<&str> {
    let at = argv
        .iter()
        .position(|a| a == "analyze" || a == "export")
        .filter(|&i| i > 0 && argv[..i].iter().any(|a| a == "sessions"))?;
    let rest = &argv[at + 1..];
    let mut i = 0;
    while i < rest.len() {
        let a = rest[i].as_str();
        if VALUE_FLAGS.contains(&a) {
            if i + 1 == rest.len() {
                return Some(a);
            }
            i += 2;
            continue;
        }
        if !a.starts_with('-') {
            return None;
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        s.split(' ').map(str::to_owned).collect()
    }

    #[test]
    fn missing_values() {
        assert_eq!(
            missing_value(&argv("pt sessions analyze --cwd")),
            Some("--cwd")
        );
        assert_eq!(
            missing_value(&argv("pt sessions export --latest --out")),
            Some("--out")
        );
        assert_eq!(missing_value(&argv("pt sessions analyze x --cwd")), None);
        assert_eq!(missing_value(&argv("pt sessions analyze --cwd /")), None);
        assert_eq!(missing_value(&argv("pt sessions ls --cwd")), None);
    }
}
