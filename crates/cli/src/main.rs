//! `peektrace`: local inspector for coding-agent sessions.

mod args;
mod argv;
mod ls;
mod memory;
mod otel;
mod render;
mod reorder;
mod search;
mod single;
mod stats;
mod stats_progress;
mod stats_render;
#[cfg(test)]
mod stats_render_tests;
mod width;
mod zone;

use std::io::{self, Write as _};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context as _;
use clap::Parser as _;
use clap::error::ErrorKind;
use peektrace_core::PathStyle;
use peektrace_core::roots::RootsEnv;
use peektrace_core::select;
use tracing::{Span, field, info_span};

use crate::args::{
    Cli, Command, MemoryArgs, MemoryCommand, SessionsArgs, SessionsCommand, StatsArgs, StatsCommand,
};
use crate::ls::{Context, Globals};

const VERSION: &str = match option_env!("PEEKTRACE_VERSION") {
    Some(v) => v,
    None => "0.0.0-dev",
};

/// Everything a command reads from the environment, read once here.
/// Values that are not valid UTF-8 are decoded lossily, as Node does.
/// `cwd_flag` is the `--cwd` value; the working dir is read only when it is
/// relative.
fn gather_context(cwd_flag: Option<&str>) -> anyhow::Result<Context> {
    let home = std::env::home_dir()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_default();
    let roots_env = RootsEnv::from_vars(home, PathStyle::NATIVE, |k| {
        std::env::var_os(k).map(|v| v.to_string_lossy().into_owned())
    });
    let settings = std::fs::read(roots_env.settings_path())
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let cwd = cwd_flag
        .filter(|c| select::needs_base_dir(c, &roots_env.home, roots_env.style))
        .map(|_| {
            std::env::current_dir()
                .context("reading the current directory")
                .map(|d| d.to_string_lossy().into_owned())
        })
        .transpose()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("reading the clock")?;
    #[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
    let now_ms = now.as_millis() as f64;
    Ok(Context {
        roots_env,
        settings,
        cwd,
        now_ms,
    })
}

/// Write `parts` to stdout, treating a closed pipe as success.
fn write_stdout(parts: &[&str]) -> io::Result<()> {
    let mut out = io::stdout().lock();
    let written = parts
        .iter()
        .try_for_each(|p| out.write_all(p.as_bytes()))
        .and_then(|()| out.flush());
    match written {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

fn print_stdout(text: &str) -> io::Result<()> {
    write_stdout(&[text, "\n"])
}

fn print_stderr(text: &str) {
    let _ = writeln!(io::stderr().lock(), "{text}");
}

/// Help for the command at `path` (empty for the root), exit 0.
fn print_help(path: &[&str]) -> ExitCode {
    let mut cmd = <Cli as clap::CommandFactory>::command();
    cmd.build();
    let help = path
        .iter()
        .try_fold(&mut cmd, |c, name| c.find_subcommand_mut(name))
        .map(|c| c.render_help().to_string());
    match help.map(|h| write_stdout(&[&h])) {
        Some(Ok(())) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

/// The command to run, from the parsed command line.
#[derive(Clone, Copy)]
enum Run<'a> {
    Ls(&'a args::LsArgs),
    Analyze(&'a args::AnalyzeArgs),
    Export(&'a args::ExportArgs),
    Search(&'a args::SearchArgs),
    MemoryLs(&'a args::MemoryLsArgs),
    MemoryShow(&'a args::MemoryNameArgs),
    MemoryRm(&'a args::MemoryNameArgs),
    Stats(&'a args::StatsCommon),
    StatsDrill(&'a args::StatsDrillArgs),
    StatsPrompt(&'a args::StatsPromptArgs),
    StatsRefresh(&'a args::StatsRefreshArgs),
    StatsUsage(&'a args::StatsUsageArgs),
}

impl<'a> Run<'a> {
    fn cwd_flag(self) -> Option<&'a str> {
        match self {
            Self::Ls(a) => a.cwd.as_deref(),
            Self::Analyze(a) => a.target.cwd.as_deref(),
            Self::Export(a) => a.target.cwd.as_deref(),
            _ => None,
        }
    }
}

/// A command under the root `cli` span. Every failure, including an early
/// `?` exit, records `error` on that span.
fn run(cli: &Cli, cmd: Run<'_>) -> anyhow::Result<ExitCode> {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let command_line = argv::loggable(&args);
    let scrub = argv::has_free_text(&args);
    let root = info_span!(
        "cli",
        peektrace.root = true,
        peektrace.kind = "cli",
        argv = command_line.as_str(),
        sessions.unreadable = field::Empty,
        error = field::Empty
    );
    let _entered = root.enter();
    let failed = Failure { root: &root, scrub };
    let result = run_command(cli, cmd, failed);
    if let Err(e) = &result {
        failed.record(&format!("{e:#}"));
    }
    result
}

/// Where a failure is recorded. On a command line holding typed free text
/// the root span gets a fixed message, since errors quote the user's words.
#[derive(Clone, Copy)]
struct Failure<'a> {
    root: &'a Span,
    scrub: bool,
}

impl Failure<'_> {
    fn record(self, message: &str) {
        let message = if self.scrub {
            "invalid command line"
        } else {
            message
        };
        self.root.record("error", field::display(message));
    }

    /// Print a user error and fail.
    fn fail(self, e: &ls::UserError) -> ExitCode {
        self.record(&e.to_string());
        print_stderr(&e.to_string());
        ExitCode::FAILURE
    }
}

fn print_warnings(cli: &Cli, warnings: &[String]) {
    if otel::shows_warnings(cli.log_level) {
        for w in warnings {
            print_stderr(&format!("WARN {w}"));
        }
    }
}

/// `Cannot write` reason for a failed export write, as TS words it.
fn write_failure(e: &io::Error) -> &'static str {
    match e.kind() {
        io::ErrorKind::NotFound => "the directory does not exist",
        io::ErrorKind::PermissionDenied => "permission denied",
        io::ErrorKind::IsADirectory => "the path is a directory",
        io::ErrorKind::AlreadyExists => "AlreadyExists",
        _ => "Unknown",
    }
}

fn run_command(cli: &Cli, cmd: Run<'_>, failure: Failure<'_>) -> anyhow::Result<ExitCode> {
    let root = failure.root;
    let ctx = gather_context(cmd.cwd_flag())?;
    let globals = Globals {
        json: cli.json,
        pretty: cli.pretty,
        read_only: cli.read_only,
    };
    let out = match cmd {
        Run::Ls(args) => match ls::run(args, globals, &ctx) {
            Ok(out) => {
                root.record("sessions.unreadable", out.unreadable);
                print_warnings(cli, &out.warnings);
                print_stdout(&out.stdout)?;
                out.notes.iter().for_each(|n| print_stderr(n));
                return Ok(ExitCode::SUCCESS);
            }
            Err(e) => return Ok(failure.fail(&e)),
        },
        Run::Analyze(args) => single::run_analyze(args, globals, &ctx),
        Run::Export(args) => single::run_export(args, globals, &ctx),
        Run::Search(args) => search::run(args, globals, &ctx),
        Run::MemoryLs(args) => Ok(memory::run_ls(args, globals, &ctx)),
        Run::MemoryShow(args) => Ok(memory::run_show(args, globals, &ctx)),
        Run::MemoryRm(args) => {
            let tmp = std::env::temp_dir().to_string_lossy().into_owned();
            memory::run_rm(args, globals, &ctx, &tmp)
        }
        Run::Stats(args) => stats::run_report(args, globals, &ctx),
        Run::StatsDrill(args) => stats::run_drill(args, globals, &ctx),
        Run::StatsPrompt(args) => stats::run_prompt(args, &ctx),
        Run::StatsRefresh(args) => stats::run_refresh(args, globals, &ctx),
        Run::StatsUsage(args) => stats::run_usage(args, globals, &ctx),
    };
    let out = match out {
        Ok(out) => out,
        Err(e) => return Ok(failure.fail(&e)),
    };
    if let Some(n) = out.unreadable {
        root.record("sessions.unreadable", n);
    }
    print_warnings(cli, &out.warnings);
    if let Some(stdout) = &out.stdout {
        print_stdout(stdout)?;
    }
    if let Some((path, text)) = &out.write {
        if let Err(e) = std::fs::write(path, text) {
            let e = ls::UserError::Invalid(format!("Cannot write {path}: {}", write_failure(&e)));
            return Ok(failure.fail(&e));
        }
        print_stderr(&format!("Wrote {path}"));
    }
    out.notes.iter().for_each(|n| print_stderr(n));
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args_os()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if let Some(flag) = args::missing_value(&argv) {
        print_stderr(&format!("Missing value for {flag}."));
        return ExitCode::FAILURE;
    }
    let cli = match Cli::try_parse_from(reorder::normalize(&argv)) {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return match e.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => ExitCode::SUCCESS,
                _ => ExitCode::FAILURE,
            };
        }
    };
    if cli.version {
        return match print_stdout(VERSION) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        };
    }
    let cmd = match &cli.command {
        None => return print_help(&[]),
        Some(Command::Sessions(SessionsArgs { command: None })) => {
            return print_help(&["sessions"]);
        }
        Some(Command::Sessions(SessionsArgs { command: Some(c) })) => match c {
            SessionsCommand::Ls(a) => Run::Ls(a),
            SessionsCommand::Analyze(a) => Run::Analyze(a),
            SessionsCommand::Export(a) => Run::Export(a),
            SessionsCommand::Search(a) => Run::Search(a),
        },
        Some(Command::Memory(MemoryArgs { command: None })) => return print_help(&["memory"]),
        Some(Command::Memory(MemoryArgs { command: Some(c) })) => match c {
            MemoryCommand::Ls(a) => Run::MemoryLs(a),
            MemoryCommand::Show(a) => Run::MemoryShow(a),
            MemoryCommand::Rm(a) => Run::MemoryRm(a),
        },
        Some(Command::Stats(StatsArgs { command, common })) => match command {
            None => Run::Stats(common),
            Some(StatsCommand::Drill(a)) => Run::StatsDrill(a),
            Some(StatsCommand::Prompt(a)) => Run::StatsPrompt(a),
            Some(StatsCommand::Refresh(a)) => Run::StatsRefresh(a),
            Some(StatsCommand::Usage(a)) => Run::StatsUsage(a),
        },
    };
    let echo = cli.otel || std::env::var("PEEKTRACE_OTEL").is_ok_and(|v| !v.is_empty());
    otel::init(echo, cli.log_level);
    run(&cli, cmd).unwrap_or_else(|e| {
        print_stderr(&format!("{e:#}"));
        ExitCode::FAILURE
    })
}
