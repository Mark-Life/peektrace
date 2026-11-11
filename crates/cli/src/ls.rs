//! `peektrace sessions ls`: validate flags, list, select and render.

use peektrace_core::js::num;
use peektrace_core::roots::{self, RootsEnv};
use peektrace_core::select::{self, SelectOptions};
use peektrace_core::{Agent, AgentRoots, ListFilter, list_sessions};

use crate::args::LsArgs;
use crate::render;

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
fn parse_limit(raw: &str) -> Result<f64, UserError> {
    let n = num::parse_number(raw);
    if peektrace_core::js::text::trim(raw).is_empty() || !num::is_safe_integer(n) {
        return Err(UserError::Validation(format!("'{raw}' is not a integer")));
    }
    Ok(n)
}

/// `--agent`: absent or `all` means every agent.
fn parse_agent(raw: Option<&str>) -> Result<Option<Agent>, UserError> {
    match raw.unwrap_or("all") {
        "all" => Ok(None),
        value => Agent::parse(value).map(Some).ok_or_else(|| {
            let known = Agent::ALL.map(Agent::as_str).join(", ");
            UserError::Invalid(format!("Unknown agent \"{value}\". Known: {known}, all."))
        }),
    }
}

/// Validated `--limit` (as a row count) and `--since` (epoch ms).
fn parse_bounds(
    limit: f64,
    since: Option<&str>,
    now_ms: f64,
) -> Result<(usize, Option<f64>), UserError> {
    if limit < 0.0 {
        return Err(UserError::Invalid(format!(
            "Invalid --limit {}. Use 0 for all, or a positive number.",
            num::to_js_string(limit)
        )));
    }
    let since_ms = since
        .map(|raw| {
            select::parse_since(raw, now_ms).ok_or_else(|| {
                UserError::Invalid(format!(
                    "Invalid --since \"{raw}\". Use an ISO date (2026-01-31) or a relative value (7d, 12h, 2w)."
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

/// Run `sessions ls` against `ctx`.
pub fn run(args: &LsArgs, globals: Globals, ctx: &Context) -> Result<Output, UserError> {
    let limit = parse_limit(&args.limit)?;
    let agent = parse_agent(args.agent.as_deref())?;
    let (limit, since_ms) = parse_bounds(limit, args.since.as_deref(), ctx.now_ms)?;
    let cwd = args
        .cwd
        .as_deref()
        .map(|c| select::resolve_cwd(c, &ctx.roots_env.home, ctx.cwd.as_deref().unwrap_or("")));
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
    let roots = AgentRoots {
        claude: roots::claude_sources(&ctx.roots_env, &settings.claude),
    };
    let filter = ListFilter {
        agent,
        project: args.project.clone(),
    };
    let listing = list_sessions(&roots, &filter);
    let unreadable = listing.unreadable.len();
    let picked = select::select_sessions(
        listing.headers,
        &SelectOptions {
            since_ms,
            cwd,
            limit,
        },
    );
    let (stdout, table_notes) = if globals.json {
        (render::json_rows(&picked.rows), Vec::new())
    } else {
        render::session_table(&picked.rows, globals.pretty)
    };
    Ok(Output {
        stdout,
        warnings,
        notes: render::list_notes(unreadable, table_notes, picked.hidden),
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
        assert_eq!(parse_bounds(-0.0, None, 0.0), Ok((0, None)));
        assert_eq!(
            parse_bounds(-1.0, Some("xx"), 0.0),
            Err(UserError::Invalid(
                "Invalid --limit -1. Use 0 for all, or a positive number.".to_owned()
            ))
        );
        assert!(parse_bounds(1.0, Some("xx"), 0.0).is_err());
    }
}
