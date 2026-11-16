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

    // Accepted for TS flag parity. No command mutates anything yet, so
    // nothing reads it.
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
