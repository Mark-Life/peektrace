//! From the command line to the command it names: owned pages first,
//! then clap, then [`crate::run`].

use std::process::ExitCode;

use clap::Parser as _;
use clap::error::ErrorKind;

use crate::args::{
    self, Cli, Command, MemoryArgs, MemoryCommand, SessionsArgs, SessionsCommand, StatsArgs,
    StatsCommand,
};
use crate::{Failure, Run, VERSION, help, print_stderr, print_stdout, run, write_stdout};

/// A page or script that runs before any command, exit 0 (1 for a
/// `--completions` value that names no shell).
pub fn print_early(early: help::Early, failure: Failure<'_>) -> ExitCode {
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

/// Parse the command line and run what it names.
pub fn dispatch(raw: &[String], argv: &[String], failure: Failure<'_>) -> ExitCode {
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
