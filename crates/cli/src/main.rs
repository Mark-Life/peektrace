//! `peektrace`: local inspector for coding-agent sessions.

mod args;
mod ls;
mod otel;
mod render;
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

use crate::args::{Cli, Command, LsArgs, SessionsArgs, SessionsCommand};
use crate::ls::{Context, Globals};

const VERSION: &str = match option_env!("PEEKTRACE_VERSION") {
    Some(v) => v,
    None => "0.0.0-dev",
};

/// Everything `sessions ls` reads from the environment, read once here.
/// Values that are not valid UTF-8 are decoded lossily, as Node does.
fn gather_context(args: &LsArgs) -> anyhow::Result<Context> {
    let home = std::env::home_dir()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_default();
    let roots_env = RootsEnv::from_vars(home, PathStyle::NATIVE, |k| {
        std::env::var_os(k).map(|v| v.to_string_lossy().into_owned())
    });
    let settings = std::fs::read(roots_env.settings_path())
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let cwd = args
        .cwd
        .as_deref()
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

/// `sessions ls` under the root `cli` span. Every failure, including an
/// early `?` exit, records `error` on that span.
fn run(cli: &Cli, args: &LsArgs) -> anyhow::Result<ExitCode> {
    let command_line = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    let root = info_span!(
        "cli",
        peektrace.root = true,
        peektrace.kind = "cli",
        argv = command_line.as_str(),
        sessions.unreadable = field::Empty,
        error = field::Empty
    );
    let _entered = root.enter();
    let result = run_ls(cli, args, &root);
    if let Err(e) = &result {
        root.record("error", field::display(format!("{e:#}")));
    }
    result
}

fn run_ls(cli: &Cli, args: &LsArgs, root: &Span) -> anyhow::Result<ExitCode> {
    let ctx = gather_context(args)?;
    let globals = Globals {
        json: cli.json,
        pretty: cli.pretty,
    };
    match ls::run(args, globals, &ctx) {
        Ok(out) => {
            root.record("sessions.unreadable", out.unreadable);
            if otel::shows_warnings(cli.log_level) {
                out.warnings
                    .iter()
                    .for_each(|w| print_stderr(&format!("WARN {w}")));
            }
            print_stdout(&out.stdout)?;
            out.notes.iter().for_each(|n| print_stderr(n));
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            root.record("error", field::display(&e));
            print_stderr(&e.to_string());
            Ok(ExitCode::FAILURE)
        }
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
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
    let args = match &cli.command {
        None => return print_help(&[]),
        Some(Command::Sessions(SessionsArgs { command: None })) => {
            return print_help(&["sessions"]);
        }
        Some(Command::Sessions(SessionsArgs {
            command: Some(SessionsCommand::Ls(args)),
        })) => args,
    };
    let echo = cli.otel || std::env::var("PEEKTRACE_OTEL").is_ok_and(|v| !v.is_empty());
    otel::init(echo, cli.log_level);
    run(&cli, args).unwrap_or_else(|e| {
        print_stderr(&format!("{e:#}"));
        ExitCode::FAILURE
    })
}
