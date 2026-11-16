//! Command-line shape. Global flags work before or after the subcommand.

use clap::builder::{OsStringValueParser, TypedValueParser};
use clap::{Args, Parser, Subcommand};

use crate::otel::LogLevel;

/// Local, loopback-only inspector for Claude Code, Codex, Pi and `OpenCode`
/// sessions: context-budget forensics, stats and Claude memory.
#[derive(Debug, Parser)]
#[expect(clippy::struct_excessive_bools, reason = "independent CLI flags")]
#[command(name = "peektrace", disable_version_flag = true)]
pub struct Cli {
    /// Emit raw JSON instead of rendered tables
    #[arg(long, global = true)]
    pub json: bool,

    /// Render aligned tables, not tab-separated text
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Refuse any mutating command (safe mode)
    #[arg(long = "read-only", global = true)]
    pub read_only: bool,

    /// Log Effect spans to stderr (or set `PEEKTRACE_OTEL`)
    #[arg(long, global = true)]
    pub otel: bool,

    /// Set the minimum log level
    #[arg(long = "log-level", global = true, value_enum)]
    pub log_level: Option<LogLevel>,

    // Telemetry is decided from the raw command line before parsing
    // (`argv::telemetry_on`); clap only accepts the flags.
    /// Disable local wide-event telemetry (on by default)
    #[arg(long = "no-telemetry", global = true, overrides_with = "telemetry")]
    pub no_telemetry: bool,

    /// Local wide-event telemetry (on by default)
    #[arg(long, global = true, overrides_with = "no_telemetry", hide = true)]
    pub telemetry: bool,

    /// Print a completion script
    #[arg(long, value_enum, value_name = "shell")]
    pub completions: Option<Shell>,

    /// Use `peektrace serve` over HTTP (`PEEKTRACE_TOKEN`)
    #[arg(long, global = true, value_parser = lossy())]
    pub remote: Option<String>,

    /// Print the version
    #[arg(short = 'v', long = "version")]
    pub version: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Shells `--completions` writes a script for; `sh` gets the bash one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Shell {
    Sh,
    Bash,
    Fish,
    Zsh,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List and analyze sessions
    Sessions(SessionsArgs),
    /// Browse and delete Claude memory entries
    Memory(MemoryArgs),
    /// Rank what fails and what costs time across all sessions
    Stats(StatsArgs),
    /// Print one-line context health for a status line
    Statusline(StatuslineArgs),
    /// Start the local web UI
    Serve(ServeArgs),
    /// Browse sessions in a terminal UI
    Tui(ServeArgs),
    /// Serve read-only session and stats tools over stdio MCP
    Mcp,
    /// Write a redacted telemetry bundle for support
    Doctor(DoctorArgs),
    /// Upgrade the installed binary to the latest release
    Upgrade(UpgradeArgs),
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Max events to include
    #[arg(long, default_value = "200", allow_hyphen_values = true, value_parser = lossy())]
    pub last: String,
    /// Only errors/slow events
    #[arg(long = "interesting-only")]
    pub interesting_only: bool,
    /// Output path for the bundle
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub out: Option<String>,
}

#[derive(Debug, Args)]
pub struct StatuslineArgs {
    /// Give up and print "ctx --" after this many ms (default 1500)
    #[arg(long, default_value = "1500", allow_hyphen_values = true, value_parser = lossy())]
    pub timeout: String,
}

#[derive(Debug, Args)]
pub struct UpgradeArgs {
    /// Install a specific release tag instead of the latest (pin or downgrade)
    #[arg(long, allow_hyphen_values = true, value_parser = lossy())]
    pub version: Option<String>,
    /// Only report whether an update is available; download and write nothing
    #[arg(long)]
    pub check: bool,
}

/// `memory` with no subcommand prints its help and exits 0.
#[derive(Debug, Args)]
pub struct MemoryArgs {
    #[command(subcommand)]
    pub command: Option<MemoryCommand>,
}

#[derive(Debug, Subcommand)]
pub enum MemoryCommand {
    /// List projects with memory, or the entries of one project
    Ls(MemoryLsArgs),
    /// Show the frontmatter and full body of one entry
    Show(MemoryNameArgs),
    /// Delete one entry (blocked under --read-only)
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
    /// Show the evidence behind one finding
    Drill(StatsDrillArgs),
    /// Print a prompt to hand the report to an agent
    Prompt(StatsPromptArgs),
    /// Rebuild the stats index
    Refresh(StatsRefreshArgs),
    /// Show token and cost usage over time
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

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Port to bind (default 4321, auto-picks if busy)
    #[arg(long, default_value_t = 4321, allow_hyphen_values = true)]
    pub port: i64,

    /// Interface to bind (default 127.0.0.1 loopback; any other host needs a
    /// token and is read-only unless --allow-write; prefer ssh -L)
    #[arg(long, default_value = "127.0.0.1", value_parser = lossy())]
    pub host: String,

    /// Open the browser on start (use --no-open to skip)
    #[arg(long, overrides_with = "no_open")]
    pub open: bool,

    /// Do not open the browser on start
    #[arg(long = "no-open", overrides_with = "open")]
    pub no_open: bool,

    /// Allow writes on a non-loopback --host (read-only by default there)
    #[arg(long = "allow-write")]
    pub allow_write: bool,

    /// Web UI to serve
    #[arg(long, value_enum, default_value_t = WebUi::Inspector)]
    pub ui: WebUi,
}

/// Which web UI `serve` hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum WebUi {
    /// The React inspector
    Inspector,
    /// The Rune app (WebGPU, experimental)
    Rune,
}

/// `sessions` with no subcommand prints its help and exits 0.
#[derive(Debug, Args)]
pub struct SessionsArgs {
    #[command(subcommand)]
    pub command: Option<SessionsCommand>,
}

#[derive(Debug, Subcommand)]
pub enum SessionsCommand {
    /// List session headers
    Ls(LsArgs),
    /// Show context forensics for one session
    Analyze(AnalyzeArgs),
    /// Export one session to HTML, Markdown or JSON
    Export(ExportArgs),
    /// Find sessions by text, redacted by default
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
    let ("sessions", "analyze" | "export", at) = crate::reorder::command_path(argv)? else {
        return None;
    };
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
        assert_eq!(
            missing_value(&argv("pt --log-level debug sessions export --out")),
            Some("--out")
        );
        assert_eq!(
            missing_value(&argv("pt sessions search export --out")),
            None
        );
        assert_eq!(
            missing_value(&argv("pt sessions search how to export --format")),
            None
        );
    }
}
