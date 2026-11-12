//! `peektrace`: local inspector for coding-agent sessions.

mod args;
mod argv;
mod doctor;
mod help;
mod ls;
mod mcp;
mod memory;
mod otel;
mod remote;
mod render;
mod reorder;
mod search;
mod serve;
mod single;
mod stats;
mod stats_progress;
mod stats_render;
#[cfg(test)]
mod stats_render_tests;
mod statusline;
mod tui;
mod upgrade;
mod verdict;
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
fn gather_context(cwd_flag: Option<&str>, always_cwd: bool) -> anyhow::Result<Context> {
    let home = std::env::home_dir()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_default();
    let roots_env = RootsEnv::from_vars(home, PathStyle::NATIVE, |k| {
        std::env::var_os(k).map(|v| v.to_string_lossy().into_owned())
    });
    let settings = std::fs::read(roots_env.settings_path())
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let cwd = (always_cwd
        || cwd_flag.is_some_and(|c| select::needs_base_dir(c, &roots_env.home, roots_env.style)))
    .then(|| {
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

/// A page or script that runs before any command, exit 0 (1 for a
/// `--completions` value that names no shell).
fn print_early(early: help::Early, failure: Failure<'_>) -> ExitCode {
    let text = match early {
        help::Early::RootHelp => format!("{}\n", help::root_page(VERSION)),
        help::Early::GroupHelp(g) => format!("{}\n", help::group_page(g, VERSION)),
        help::Early::Completions(Ok(shell)) => help::completions(shell),
        help::Early::Completions(Err(value)) => {
            let message = format!(
                "invalid value '{value}' for '--completions <shell>'\n  [possible values: sh, bash, fish, zsh]"
            );
            failure.record(&message);
            print_stderr(&message);
            return ExitCode::FAILURE;
        }
    };
    match write_stdout(&[&text]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
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
    Serve(&'a args::ServeArgs),
    Statusline(&'a args::StatuslineArgs),
    Mcp,
    Upgrade(&'a args::UpgradeArgs),
    Doctor(&'a args::DoctorArgs),
    Tui(&'a args::ServeArgs),
}

impl<'a> Run<'a> {
    /// The command name when it has no `--remote` form in this build.
    const fn local_only(&self) -> Option<&'static str> {
        match self {
            Self::Search(_) => Some("sessions search"),
            Self::MemoryLs(_) => Some("memory ls"),
            Self::MemoryShow(_) => Some("memory show"),
            Self::MemoryRm(_) => Some("memory rm"),
            Self::Stats(_) => Some("stats"),
            Self::StatsDrill(_) => Some("stats drill"),
            Self::StatsPrompt(_) => Some("stats prompt"),
            Self::StatsRefresh(_) => Some("stats refresh"),
            Self::StatsUsage(_) => Some("stats usage"),
            Self::Ls(_)
            | Self::Analyze(_)
            | Self::Export(_)
            | Self::Serve(_)
            | Self::Statusline(_)
            | Self::Mcp
            | Self::Upgrade(_)
            | Self::Doctor(_)
            | Self::Tui(_) => None,
        }
    }

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
fn run(cli: &Cli, cmd: Run<'_>, failure: Failure<'_>) -> anyhow::Result<ExitCode> {
    let result = run_command(cli, cmd, failure);
    if let Err(e) = &result {
        failure.record(&format!("{e:#}"));
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
            message.trim_end()
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
    if let (Some(_), Some(name)) = (&cli.remote, cmd.local_only()) {
        let e = ls::UserError::Invalid(format!(
            "--remote is not supported for `{name}` in the Rust build yet."
        ));
        return Ok(failure.fail(&e));
    }
    if let Run::Mcp = cmd {
        if cli.remote.is_some() {
            let e = ls::UserError::Invalid(
                "mcp runs locally only; remote is not supported yet".to_owned(),
            );
            return Ok(failure.fail(&e));
        }
        let ctx = gather_context(None, true)?;
        print_warnings(cli, &warnings_of(&ctx));
        mcp::serve(ctx, io::stdin().lock());
        return Ok(ExitCode::SUCCESS);
    }
    if let Run::Doctor(args) = cmd {
        return Ok(run_doctor(args, failure));
    }
    if let Run::Upgrade(args) = cmd {
        let env = upgrade::UpgradeEnv::from_process(VERSION);
        let mut print = |line: &str| {
            let _ = print_stdout(line);
        };
        return Ok(match upgrade::run(args, &env, &mut print) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => failure.fail(&e),
        });
    }
    if let Run::Statusline(args) = cmd {
        let deadline = match statusline::parse_timeout(&args.timeout) {
            Ok(d) => d,
            Err(e) => return Ok(failure.fail(&e)),
        };
        let ctx = gather_context(None, false)?;
        print_warnings(cli, &warnings_of(&ctx));
        let remote = cli.remote.as_deref().map(remote_of);
        print_stdout(&statusline::run(ctx, remote, deadline))?;
        return Ok(ExitCode::SUCCESS);
    }
    let ctx = gather_context(cmd.cwd_flag(), false)?;
    let globals = Globals {
        json: cli.json,
        pretty: cli.pretty,
        read_only: cli.read_only,
    };
    let remote = cli.remote.as_deref().map(remote_of);
    let remote = remote.as_ref();
    let out = match cmd {
        Run::Serve(args) => return run_serve(cli, args, &ctx, failure),
        Run::Tui(args) => return run_tui(cli, args, &ctx, failure),
        Run::Ls(args) => match ls::run(args, globals, &ctx, remote) {
            Ok(out) => {
                root.record("sessions.unreadable", out.unreadable);
                print_warnings(cli, &out.warnings);
                print_stdout(&out.stdout)?;
                out.notes.iter().for_each(|n| print_stderr(n));
                return Ok(ExitCode::SUCCESS);
            }
            Err(e) => return Ok(failure.fail(&e)),
        },
        Run::Analyze(args) => single::run_analyze(args, globals, &ctx, remote),
        Run::Export(args) => single::run_export(args, globals, &ctx, remote),
        Run::Search(args) => search::run(args, globals, &ctx),
        Run::MemoryLs(args) => Ok(memory::run_ls(args, globals, &ctx)),
        Run::MemoryShow(args) => Ok(memory::run_show(args, globals, &ctx)),
        Run::MemoryRm(args) => {
            let tmp = peektrace_core::js::os::tmpdir();
            memory::run_rm(args, globals, &ctx, &tmp)
        }
        Run::Stats(args) => stats::run_report(args, globals, &ctx),
        Run::StatsDrill(args) => stats::run_drill(args, globals, &ctx),
        Run::StatsPrompt(args) => stats::run_prompt(args, &ctx),
        Run::StatsRefresh(args) => stats::run_refresh(args, globals, &ctx),
        Run::StatsUsage(args) => stats::run_usage(args, globals, &ctx),
        Run::Statusline(_) | Run::Mcp | Run::Upgrade(_) | Run::Doctor(_) => unreachable!(),
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

fn remote_of(url: &str) -> remote::Remote {
    remote::Remote::new(
        url,
        std::env::var(peektrace_server::auth::TOKEN_ENV)
            .ok()
            .as_deref(),
    )
}

/// The unparseable-settings warning, for commands that print no other
/// output of the listing.
fn warnings_of(ctx: &Context) -> Vec<String> {
    single::agent_roots(ctx).1
}

/// What the startup update check reads.
fn notice_env(ctx: &Context) -> upgrade::notice::NoticeEnv {
    let var = |k: &str| std::env::var(k).ok();
    upgrade::notice::NoticeEnv {
        version: VERSION.to_owned(),
        exe: upgrade::notice::exe_path(),
        cache_path: upgrade::notice::cache_path(
            var("PEEKTRACE_DIR").as_deref(),
            &ctx.roots_env.home,
        ),
        config: upgrade::release::ReleaseConfig::from_env(upgrade::release::BUILD_FEED, var),
        now_ms: ctx.now_ms,
        disabled: var("PEEKTRACE_NO_UPDATE_CHECK").is_some_and(|v| !v.is_empty()),
    }
}

/// `serve`: print the banner once bound, then serve until stopped.
fn run_serve(
    cli: &Cli,
    args: &args::ServeArgs,
    ctx: &Context,
    failure: Failure<'_>,
) -> anyhow::Result<ExitCode> {
    let prepared = match serve::prepare(args, cli.read_only, ctx) {
        Ok(p) => p,
        Err(e) => return Ok(failure.fail(&e)),
    };
    print_warnings(cli, &prepared.warnings);
    let banner = prepared.banner.clone();
    let desktop = std::env::var("PEEKTRACE_CLIENT").is_ok_and(|v| v == "desktop");
    serve::run(prepared, |watch| {
        let _ = print_stdout(&banner.stdout.join("\n"));
        banner
            .stderr
            .iter()
            .chain(watch)
            .for_each(|l| print_stderr(l));
        if desktop {
            let _ = print_stdout(&format!("PEEKTRACE_READY:{}", banner.port));
        }
        upgrade::notice::spawn_notice(notice_env(ctx), |line| {
            let _ = print_stdout(line);
        });
    })?;
    Ok(ExitCode::SUCCESS)
}

/// `tui`: the terminal UI, with the web server beside it.
fn run_tui(
    cli: &Cli,
    args: &args::ServeArgs,
    ctx: &Context,
    failure: Failure<'_>,
) -> anyhow::Result<ExitCode> {
    let prepared = match serve::prepare(args, cli.read_only, ctx) {
        Ok(p) => p,
        Err(e) => return Ok(failure.fail(&e)),
    };
    if let Err(e) = tui::check_terminal() {
        return Ok(failure.fail(&e));
    }
    print_warnings(cli, &prepared.warnings);
    tui::run(prepared, ctx)?;
    Ok(ExitCode::SUCCESS)
}

/// `doctor`: write the support bundle and print where it went.
fn run_doctor(args: &args::DoctorArgs, failure: Failure<'_>) -> ExitCode {
    let home = std::env::home_dir()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_default();
    let peektrace_dir = std::env::var_os("PEEKTRACE_DIR").map(|d| d.to_string_lossy().into_owned());
    let env = doctor::DoctorEnv {
        peektrace_dir: peektrace_dir.as_deref(),
        home: &home,
        now_ms: now_ms(),
    };
    match doctor::run(args, &env) {
        Ok(lines) => match print_stdout(&lines.join("\n")) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        },
        Err(e) => failure.fail(&e),
    }
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
        .floor()
}

/// Start the subscriber: the `--otel` echo, log lines and the telemetry
/// store, each decided from the raw command line as TS does.
fn start_tracing(raw: &[String]) {
    let flags = argv::before_separator(raw);
    let echo = flags.iter().any(|a| a == "--otel")
        || std::env::var("PEEKTRACE_OTEL").is_ok_and(|v| !v.is_empty());
    let on = argv::telemetry_on(raw, std::env::var_os("PEEKTRACE_NO_TELEMETRY").is_some());
    let store = on.then(|| {
        let home = std::env::home_dir()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dir = std::env::var_os("PEEKTRACE_DIR").map(|d| d.to_string_lossy().into_owned());
        let dir = peektrace_telemetry::store::dir_of(dir.as_deref(), &home).ok();
        #[expect(clippy::cast_possible_truncation, reason = "epoch ms fit i64")]
        let now = now_ms() as i64;
        peektrace_telemetry::start(dir, VERSION, now)
    });
    otel::init(echo, argv::log_level(raw), store);
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args_os()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    start_tracing(&raw);
    let argv = reorder::normalize(&raw);
    let command_line = argv::loggable(&argv::version_alias(&argv));
    let code = {
        let root = info_span!(
            "cli",
            peektrace.root = true,
            peektrace.kind = "cli",
            argv = command_line.as_str(),
            sessions.unreadable = field::Empty,
            error = field::Empty
        );
        let _entered = root.enter();
        let failure = Failure {
            root: &root,
            scrub: argv::has_free_text(&argv),
        };
        dispatch(&raw, &argv, failure)
    };
    peektrace_telemetry::flush(peektrace_telemetry::EXIT_WAIT);
    code
}

/// Parse the command line and run what it names.
fn dispatch(raw: &[String], argv: &[String], failure: Failure<'_>) -> ExitCode {
    if let Some(early) = help::early(raw) {
        return print_early(early, failure);
    }
    if let Some(flag) = args::missing_value(raw) {
        let message = format!("Missing value for {flag}.");
        failure.record(&message);
        print_stderr(&message);
        return ExitCode::FAILURE;
    }
    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return match e.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => ExitCode::SUCCESS,
                _ => {
                    failure.record(&e.to_string());
                    ExitCode::FAILURE
                }
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
        None => {
            return match print_stdout(&help::banner()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(_) => ExitCode::FAILURE,
            };
        }
        Some(Command::Sessions(SessionsArgs { command: None })) => {
            return print_early(help::Early::GroupHelp("sessions"), failure);
        }
        Some(Command::Sessions(SessionsArgs { command: Some(c) })) => match c {
            SessionsCommand::Ls(a) => Run::Ls(a),
            SessionsCommand::Analyze(a) => Run::Analyze(a),
            SessionsCommand::Export(a) => Run::Export(a),
            SessionsCommand::Search(a) => Run::Search(a),
        },
        Some(Command::Memory(MemoryArgs { command: None })) => {
            return print_early(help::Early::GroupHelp("memory"), failure);
        }
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
        Some(Command::Serve(a)) => Run::Serve(a),
        Some(Command::Statusline(a)) => Run::Statusline(a),
        Some(Command::Mcp) => Run::Mcp,
        Some(Command::Upgrade(a)) => Run::Upgrade(a),
        Some(Command::Doctor(a)) => Run::Doctor(a),
        Some(Command::Tui(a)) => Run::Tui(a),
    };
    run(&cli, cmd, failure).unwrap_or_else(|e| {
        print_stderr(&format!("{e:#}"));
        ExitCode::FAILURE
    })
}
