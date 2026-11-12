//! `peektrace sessions ls`: validate flags, list, select and render.

use peektrace_core::js::num;
use peektrace_core::roots::{self, RootsEnv};
use peektrace_core::select::{self, SelectOptions};
use peektrace_core::sessions::read::listing_from_json;
use peektrace_core::{Agent, AgentRoots, ListFilter, SessionHeader, list_sessions};

use crate::args::LsArgs;
use crate::remote::Remote;
use crate::render;
use crate::zone::SystemZone;

/// What the process reads from its surroundings, gathered by `main`.
#[derive(Debug, Clone)]
pub struct Context {
    pub roots_env: RootsEnv,
    /// `settings.json` text, when the file could be read.
    pub settings: Option<String>,
    /// The working directory, read only when `--cwd` is relative.
    pub cwd: Option<String>,
    pub now_ms: f64,
}

/// Output flags after merging global and local positions.
#[derive(Debug, Clone, Copy)]
pub struct Globals {
    pub json: bool,
    pub pretty: bool,
    pub read_only: bool,
}

/// A failure the user caused. Exit code 1 with the message on stderr.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UserError {
    /// Flag value of the wrong type; printed with a blank line after it.
    #[error("{0}\n")]
    Validation(String),
    #[error("{0}")]
    Invalid(String),
}

/// What the command prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub stdout: String,
    /// Log lines at warning level, printed before `stdout`.
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
    pub unreadable: usize,
}

/// `--limit` as `@effect/cli` reads an integer: `Number(s)` that is a safe
/// integer, with blank input rejected.
pub(crate) fn parse_limit(raw: &str) -> Result<f64, UserError> {
    let n = num::parse_number(raw);
    if peektrace_core::js::text::trim(raw).is_empty() || !num::is_safe_integer(n) {
        return Err(UserError::Validation(format!("'{raw}' is not a integer")));
    }
    Ok(n)
}

/// `--agent`: absent or `all` means every agent.
pub(crate) fn parse_agent(raw: Option<&str>) -> Result<Option<Agent>, UserError> {
    let value = raw.unwrap_or("all");
    if value == "all" {
        return Ok(None);
    }
    let Some(agent) = Agent::parse(value) else {
        let known = Agent::ALL.map(Agent::as_str).join(", ");
        return Err(UserError::Invalid(format!(
            "Unknown agent \"{value}\". Known: {known}, all."
        )));
    };
    Ok(Some(agent))
}

/// How messages name the limit and since inputs: CLI flags or MCP tool
/// arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputNames {
    pub limit: &'static str,
    pub since: &'static str,
}

pub const CLI_NAMES: InputNames = InputNames {
    limit: "--limit",
    since: "--since",
};

/// Validated `--limit` (as a row count) and `--since` (epoch ms).
pub(crate) fn parse_bounds(
    limit: f64,
    since: Option<&str>,
    now_ms: f64,
    names: InputNames,
) -> Result<(usize, Option<f64>), UserError> {
    if limit < 0.0 {
        return Err(UserError::Invalid(format!(
            "Invalid {} {}. Use 0 for all, or a positive number.",
            names.limit,
            num::to_js_string(limit)
        )));
    }
    let since_ms = since
        .map(|raw| {
            select::parse_since(raw, now_ms).ok_or_else(|| {
                UserError::Invalid(format!(
                    "Invalid {} \"{raw}\". Use an ISO date (2026-01-31) or a relative value (7d, 12h, 2w).",
                    names.since
                ))
            })
        })
        .transpose()?;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a safe, non-negative integer"
    )]
    Ok((limit as usize, since_ms))
}

/// The listing: from the remote server, or read here with the settings
/// warning.
fn fetch(
    agent: Option<Agent>,
    project: Option<&str>,
    ctx: &Context,
    remote: Option<&Remote>,
) -> Result<(Vec<SessionHeader>, usize, Vec<String>), UserError> {
    if let Some(r) = remote {
        let query: Vec<(&str, &str)> = [("agent", agent.map(Agent::as_str)), ("project", project)]
            .into_iter()
            .filter_map(|(k, v)| v.map(|v| (k, v)))
            .collect();
        let (headers, unreadable) =
            listing_from_json(&r.get_json(&["sessions"], &query)?).ok_or_else(|| r.unexpected())?;
        return Ok((headers, unreadable, Vec::new()));
    }
    let settings = ctx
        .settings
        .as_deref()
        .map(roots::parse_settings)
        .unwrap_or_default();
    let warnings = settings
        .unparseable
        .then(|| {
            format!(
                "peektrace: ignoring unparseable settings at {}",
                ctx.roots_env.settings_path()
            )
        })
        .into_iter()
        .collect();
    let roots = AgentRoots::resolve(&ctx.roots_env, &settings);
    let filter = ListFilter {
        agent,
        project: project.map(str::to_owned),
    };
    let listing = list_sessions(&roots, &filter);
    Ok((listing.headers, listing.unreadable.len(), warnings))
}

/// Typed `sessions ls` inputs, shared by the CLI and the MCP tool.
#[derive(Debug, Clone, Copy, Default)]
pub struct LsInput<'a> {
    pub agent: Option<Agent>,
    pub project: Option<&'a str>,
    pub since: Option<&'a str>,
    pub cwd: Option<&'a str>,
    pub limit: f64,
}

/// Run `sessions ls` against `ctx`, or against `remote` when set.
pub fn run(
    args: &LsArgs,
    globals: Globals,
    ctx: &Context,
    remote: Option<&Remote>,
) -> Result<Output, UserError> {
    let limit = parse_limit(&args.limit)?;
    let agent = parse_agent(args.agent.as_deref())?;
    let input = LsInput {
        agent,
        project: args.project.as_deref(),
        since: args.since.as_deref(),
        cwd: args.cwd.as_deref(),
        limit,
    };
    list(&input, CLI_NAMES, globals, ctx, remote)
}

/// The listing for typed inputs; `names` words the messages and notes.
pub fn list(
    input: &LsInput<'_>,
    names: InputNames,
    globals: Globals,
    ctx: &Context,
    remote: Option<&Remote>,
) -> Result<Output, UserError> {
    let (limit, since_ms) = parse_bounds(input.limit, input.since, ctx.now_ms, names)?;
    let cwd = input.cwd.map(|c| {
        let env = &ctx.roots_env;
        select::resolve_cwd(c, &env.home, ctx.cwd.as_deref().unwrap_or(""), env.style)
    });
    let (headers, unreadable, warnings) = fetch(input.agent, input.project, ctx, remote)?;
    let picked = select::select_sessions(
        headers,
        &SelectOptions {
            since_ms,
            cwd,
            limit,
        },
        &SystemZone::default(),
    );
    let (stdout, table_notes) = if globals.json {
        (render::json_rows(&picked.rows), Vec::new())
    } else {
        render::session_table(&picked.rows, globals.pretty)
    };
    let notes = render::list_notes(unreadable, table_notes, picked.hidden, names.limit);
    Ok(Output {
        stdout,
        warnings,
        notes,
        unreadable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_parsing() {
        assert_eq!(parse_limit("30"), Ok(30.0));
        assert_eq!(parse_limit("1e1"), Ok(10.0));
        assert_eq!(
            parse_limit("abc"),
            Err(UserError::Validation("'abc' is not a integer".to_owned()))
        );
        assert!(parse_limit(" ").is_err());
        assert!(parse_limit("1.5").is_err());
        assert!(parse_limit("NaN").is_err());
    }

    #[test]
    fn agent_parsing() {
        assert_eq!(parse_agent(None), Ok(None));
        assert_eq!(parse_agent(Some("claude")), Ok(Some(Agent::Claude)));
        assert_eq!(parse_agent(Some("pi")), Ok(Some(Agent::Pi)));
        assert_eq!(parse_agent(Some("opencode")), Ok(Some(Agent::Opencode)));
        assert_eq!(
            parse_agent(Some("bogus")),
            Err(UserError::Invalid(
                "Unknown agent \"bogus\". Known: claude, codex, pi, opencode, all.".to_owned()
            ))
        );
    }

    #[test]
    fn validation_message_has_blank_line() {
        assert_eq!(
            UserError::Validation("'abc' is not a integer".to_owned()).to_string(),
            "'abc' is not a integer\n"
        );
    }

    #[test]
    fn bounds() {
        assert_eq!(parse_bounds(-0.0, None, 0.0, CLI_NAMES), Ok((0, None)));
        assert_eq!(
            parse_bounds(-1.0, Some("xx"), 0.0, CLI_NAMES),
            Err(UserError::Invalid(
                "Invalid --limit -1. Use 0 for all, or a positive number.".to_owned()
            ))
        );
        assert!(parse_bounds(1.0, Some("xx"), 0.0, CLI_NAMES).is_err());
    }
}
