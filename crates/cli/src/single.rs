//! `peektrace sessions analyze` and `sessions export`: pick the target
//! session, then render its analysis or an export.

use peektrace_core::export::{ExportFormat, ExportOptions, render_export};
use peektrace_core::js::time::{self, LocalZone};
use peektrace_core::js::{num, text};
use peektrace_core::roots::{self, RootsEnv, Source};
use peektrace_core::select::{self, SelectOptions};
use peektrace_core::sessions::model::{AnalyzedSession, SessionCost};
use peektrace_core::sessions::service::{SessionEnv, analyze_session, get_session};
use peektrace_core::sessions::write::analyzed_json;
use peektrace_core::{Agent, AgentRoots, ListFilter, list_sessions};

use crate::args::{AnalyzeArgs, ExportArgs, TargetArgs};
use crate::ls::{Context, Globals, UserError};
use crate::render;
use crate::zone::SystemZone;

/// Fraction at or above which a session is judged to be rotting.
const ROTTING_FRACTION: f64 = 0.7;

/// What a single-session command produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// Printed with a trailing newline.
    pub stdout: Option<String>,
    /// A file to write: path and text.
    pub write: Option<(String, String)>,
    pub warnings: Vec<String>,
    /// Sessions the `--latest` listing could not read.
    pub unreadable: Option<usize>,
    /// Printed on stderr after everything else.
    pub notes: Vec<String>,
}

/// The validated target: an explicit id, or the newest session.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Plan {
    Picked(String),
    Newest(Option<String>),
}

fn plan_target(
    id: Option<&str>,
    latest: bool,
    cwd: Option<&str>,
    parent: Option<&str>,
) -> Result<Plan, UserError> {
    let invalid = |m: &str| Err(UserError::Invalid(m.to_owned()));
    let wants_newest = latest || cwd.is_some();
    if let Some(id) = id.filter(|i| i.starts_with('-')) {
        return Err(UserError::Invalid(format!("Missing value for {id}.")));
    }
    if parent.is_some() && id.is_none() {
        return invalid("--parent needs an explicit subagent id.");
    }
    match id {
        Some(_) if wants_newest => invalid("Pass either a session id or --latest/--cwd, not both."),
        Some(id) => Ok(Plan::Picked(id.to_owned())),
        None if !wants_newest => invalid("Pass a session id, --latest, or --cwd <dir>."),
        None => Ok(Plan::Newest(cwd.map(str::to_owned))),
    }
}

/// Claude roots from the environment and `settings.json`, plus the warning
/// for unparseable settings.
fn claude_roots(ctx: &Context) -> (Vec<Source>, Vec<String>) {
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
    (
        roots::claude_sources(&ctx.roots_env, &settings.claude),
        warnings,
    )
}

/// Agents this build cannot read whose sessions root holds something.
fn unsearched_agents(env: &RootsEnv) -> Vec<Agent> {
    roots::unread_roots(env)
        .into_iter()
        .filter(|(_, root)| std::fs::read_dir(root).is_ok_and(|mut d| d.next().is_some()))
        .map(|(agent, _)| agent)
        .collect()
}

/// The picked session came from an id prefix; a path never does.
fn by_prefix(typed: &str, resolved: &str) -> bool {
    let id = typed.strip_suffix(".jsonl").unwrap_or(typed);
    !id.contains(['/', '\\']) && id != resolved
}

/// Note for a pick that other agents' sessions could have changed: the
/// newest session, or a top-level id prefix.
fn search_note(t: &TargetArgs, typed: &str, resolved: &str, unsearched: &[Agent]) -> Vec<String> {
    let open_ended = t.id.is_none() || (t.parent.is_none() && by_prefix(typed, resolved));
    open_ended
        .then(|| render::skipped_note(unsearched, "searched"))
        .flatten()
        .into_iter()
        .collect()
}

/// The session id to run on, and how many listed sessions were unreadable
/// when the newest one had to be found.
fn resolve_target(
    t: &TargetArgs,
    ctx: &Context,
    claude: &[Source],
    zone: &dyn LocalZone,
) -> Result<(String, Option<usize>), UserError> {
    let cwd = t.cwd.as_deref().map(|c| {
        let env = &ctx.roots_env;
        select::resolve_cwd(c, &env.home, ctx.cwd.as_deref().unwrap_or(""), env.style)
    });
    match plan_target(
        t.id.as_deref(),
        t.latest,
        cwd.as_deref(),
        t.parent.as_deref(),
    )? {
        Plan::Picked(id) => Ok((id, None)),
        Plan::Newest(cwd) => {
            let listing = list_sessions(
                &AgentRoots {
                    claude: claude.to_vec(),
                },
                &ListFilter::default(),
            );
            let unreadable = listing.unreadable.len();
            let picked = select::select_sessions(
                listing.headers,
                &SelectOptions {
                    since_ms: None,
                    cwd: cwd.clone(),
                    limit: 1,
                },
                zone,
            );
            match picked.rows.into_iter().next() {
                Some(h) => Ok((h.id.as_str().to_owned(), Some(unreadable))),
                None => Err(UserError::Invalid(match cwd {
                    Some(c) => format!("No sessions found under {c}."),
                    None => "No sessions found.".to_owned(),
                })),
            }
        }
    }
}

/// Compact token count: `950`, `12.3k`, `32.4M`.
fn tokens(value: f64) -> String {
    let n = num::js_round(value);
    if n < 1000.0 {
        num::to_js_string(n)
    } else if n < 1_000_000.0 {
        format!("{}k", num::to_fixed(n / 1000.0, 1))
    } else {
        format!("{}M", num::to_fixed(n / 1_000_000.0, 1))
    }
}

fn percent(fraction: f64) -> String {
    format!("{}%", num::to_fixed(fraction * 100.0, 1))
}

/// Share of the window, 0 when the window is unknown.
fn fraction_of(tokens: f64, window: f64) -> f64 {
    if window > 0.0 { tokens / window } else { 0.0 }
}

fn verdict(fraction: f64, dumb_zone: f64) -> &'static str {
    if fraction < dumb_zone {
        "Healthy"
    } else if fraction < ROTTING_FRACTION {
        "Degrading"
    } else {
        "Rotting"
    }
}

fn row(k: &str, v: String) -> Vec<String> {
    vec![k.to_owned(), v]
}

/// Summary rows for a session's cost; `-` when nothing is priced.
fn cost_rows(cost: Option<&SessionCost>, provider: &str) -> Vec<Vec<String>> {
    let total = render::usd_cell(cost.and_then(|c| c.total_usd));
    let from_agent = cost.is_some_and(|c| c.from_agent && c.total_usd.is_some());
    let subagents = cost.and_then(|c| c.subagents_usd).unwrap_or(0.0);
    let unpriced = cost
        .and_then(|c| c.unpriced_models.as_deref())
        .unwrap_or_default();
    let unreadable = cost.and_then(|c| c.unreadable_subagents).unwrap_or(0);
    let mut rows = vec![row(
        "est. cost",
        if from_agent {
            format!("{total} ({provider})")
        } else {
            total
        },
    )];
    if subagents > 0.0 {
        rows.push(row("  subagents", render::usd_cell(Some(subagents))));
    }
    if !unpriced.is_empty() {
        rows.push(row("unpriced models", unpriced.join(", ")));
    }
    if unreadable > 0 {
        rows.push(row(
            "unreadable subagents",
            format!("{unreadable} (not in est. cost)"),
        ));
    }
    rows
}

/// The `sessions analyze` summary and budget tables.
fn render_analysis(a: &AnalyzedSession, pretty: bool) -> String {
    let fraction = fraction_of(a.peak_context_tokens, a.context_window);
    let cross = if a.dumb_zone_cross_turn < 0 {
        "never".to_owned()
    } else {
        (a.dumb_zone_cross_turn + 1).to_string()
    };
    let mut rows = vec![
        row("session", a.parsed.session_id.clone()),
        row(
            "verdict",
            verdict(fraction, a.dumb_zone_fraction).to_owned(),
        ),
        row(
            "peak context",
            format!(
                "{} / {} ({})",
                tokens(a.peak_context_tokens),
                tokens(a.context_window),
                percent(fraction)
            ),
        ),
        row("final context", tokens(a.final_context_tokens)),
        row("turns", a.turn_count.to_string()),
        row("tool calls", a.tool_call_count.to_string()),
    ];
    rows.extend(cost_rows(a.cost.as_ref(), a.parsed.provider.as_str()));
    rows.push(row("dumb-zone cross turn", cross));
    let rows: Vec<Vec<String>> = rows
        .into_iter()
        .map(|r| r.iter().map(|c| text::to_display(c).into_owned()).collect())
        .collect();
    let summary = render::table(&["FIELD", "VALUE"], &rows, pretty);
    let budget_rows: Vec<Vec<String>> = a
        .budget
        .iter()
        .filter(|s| s.tokens > 0.0)
        .map(|s| {
            vec![
                s.label.to_owned(),
                tokens(s.tokens),
                if a.peak_context_tokens > 0.0 {
                    percent(s.tokens / a.peak_context_tokens)
                } else {
                    "-".to_owned()
                },
            ]
        })
        .collect();
    let budget = render::table(&["BUDGET (AT PEAK)", "TOKENS", "%"], &budget_rows, pretty);
    format!("{summary}\n\n{budget}")
}

/// `sessions analyze`.
pub fn run_analyze(
    args: &AnalyzeArgs,
    globals: Globals,
    ctx: &Context,
) -> Result<Output, UserError> {
    let (claude, warnings) = claude_roots(ctx);
    let unsearched = unsearched_agents(&ctx.roots_env);
    let zone = SystemZone::default();
    let (target, unreadable) = resolve_target(&args.target, ctx, &claude, &zone)?;
    let env = SessionEnv {
        claude: &claude,
        unsearched: &unsearched,
        local: &zone,
    };
    let a = analyze_session(env, &target, args.target.parent.as_deref(), true)
        .map_err(|e| UserError::Invalid(e.to_string()))?;
    let notes = search_note(&args.target, &target, &a.parsed.session_id, &unsearched);
    let stdout = if globals.json {
        analyzed_json(&a)
    } else {
        render_analysis(&a, globals.pretty)
    };
    Ok(Output {
        stdout: Some(stdout),
        write: None,
        warnings,
        unreadable,
        notes,
    })
}

/// `path.extname`: the last `.` part of the base name, `""` for dotfiles.
fn extname(path: &str) -> &str {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match base.rfind('.') {
        Some(at) if at > 0 => &base[at..],
        _ => "",
    }
}

/// `--format`, else JSON for `--json`, else the `--out` extension, else md.
fn pick_format(format: Option<ExportFormat>, json: bool, out: Option<&str>) -> ExportFormat {
    if let Some(f) = format {
        return f;
    }
    if json {
        return ExportFormat::Json;
    }
    match out.map(|o| extname(o).to_lowercase()).as_deref() {
        Some(".html" | ".htm") => ExportFormat::Html,
        Some(".json") => ExportFormat::Json,
        _ => ExportFormat::Md,
    }
}

/// `sessions export`.
pub fn run_export(args: &ExportArgs, globals: Globals, ctx: &Context) -> Result<Output, UserError> {
    let format = args
        .format
        .as_deref()
        .map(|f| {
            ExportFormat::parse(f).ok_or_else(|| {
                UserError::Validation(
                    "Expected one of the following cases: html, md, json".to_owned(),
                )
            })
        })
        .transpose()?;
    let (claude, warnings) = claude_roots(ctx);
    let unsearched = unsearched_agents(&ctx.roots_env);
    let zone = SystemZone::default();
    let (target, unreadable) = resolve_target(&args.target, ctx, &claude, &zone)?;
    let env = SessionEnv {
        claude: &claude,
        unsearched: &unsearched,
        local: &zone,
    };
    let redact = !args.no_redact;
    let session = get_session(env, &target, args.target.parent.as_deref(), redact)
        .map_err(|e| UserError::Invalid(e.to_string()))?;
    let notes = search_note(&args.target, &target, &session.session_id, &unsearched);
    let out_path = args.out.as_deref().filter(|p| *p != "-");
    #[expect(clippy::cast_possible_truncation, reason = "epoch ms fit i64")]
    let exported_at = time::to_iso_string(ctx.now_ms as i64);
    let rendered = render_export(
        pick_format(format, globals.json, out_path),
        &session,
        &ExportOptions {
            exported_at,
            home: Some(ctx.roots_env.home.clone()),
            redacted: redact,
        },
    );
    let rendered = text::to_display(&rendered).into_owned();
    Ok(match out_path {
        None => Output {
            stdout: Some(text::trim_end(&rendered).to_owned()),
            write: None,
            warnings,
            unreadable,
            notes,
        },
        Some(path) => Output {
            stdout: None,
            write: Some((path.to_owned(), rendered)),
            warnings,
            unreadable,
            notes,
        },
    })
}

#[cfg(test)]
#[path = "single_tests.rs"]
mod tests;
