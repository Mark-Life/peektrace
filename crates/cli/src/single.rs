//! `peektrace sessions analyze` and `sessions export`: pick the target
//! session, then render its analysis or an export.

use peektrace_core::export::{ExportFormat, ExportOptions, render_export};
use peektrace_core::js::time::{self, LocalZone};
use peektrace_core::js::value::JsValue;
use peektrace_core::js::{num, text};
use peektrace_core::roots;
use peektrace_core::select::{self, SelectOptions};
use peektrace_core::sessions::model::{AnalyzedSession, SessionCost};
use peektrace_core::sessions::read::{cost_from_json, listing_from_json, parsed_from_json};
use peektrace_core::sessions::service::{SessionEnv, Target, analyze_session, get_session};
use peektrace_core::sessions::write::analyzed_json;
use peektrace_core::{AgentRoots, ListFilter, SessionHeader, list_sessions};

use crate::args::{AnalyzeArgs, ExportArgs, TargetArgs};
use crate::ls::{Context, Globals, UserError};
use crate::remote::Remote;
use crate::render;
use crate::verdict::{fraction_of, verdict};
use crate::zone::SystemZone;

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

/// Messages for invalid target combinations, worded for the caller's inputs.
#[derive(Debug, Clone, Copy)]
pub struct TargetMessages {
    pub parent_without_id: &'static str,
    pub id_and_newest: &'static str,
    pub no_target: &'static str,
}

pub const CLI_TARGET: TargetMessages = TargetMessages {
    parent_without_id: "--parent needs an explicit subagent id.",
    id_and_newest: "Pass either a session id or --latest/--cwd, not both.",
    no_target: "Pass a session id, --latest, or --cwd <dir>.",
};

fn plan_target(
    id: Option<&str>,
    latest: bool,
    cwd: Option<&str>,
    parent: Option<&str>,
    m: &TargetMessages,
) -> Result<Plan, UserError> {
    let invalid = |m: &str| Err(UserError::Invalid(m.to_owned()));
    let wants_newest = latest || cwd.is_some();
    if let Some(id) = id.filter(|i| i.starts_with('-')) {
        return Err(UserError::Invalid(format!("Missing value for {id}.")));
    }
    if parent.is_some() && id.is_none() {
        return invalid(m.parent_without_id);
    }
    match id {
        Some(_) if wants_newest => invalid(m.id_and_newest),
        Some(id) => Ok(Plan::Picked(id.to_owned())),
        None if !wants_newest => invalid(m.no_target),
        None => Ok(Plan::Newest(cwd.map(str::to_owned))),
    }
}

/// Every agent's roots from the environment and `settings.json`, plus the
/// warning for unparseable settings.
pub fn agent_roots(ctx: &Context) -> (AgentRoots, Vec<String>) {
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
    (AgentRoots::resolve(&ctx.roots_env, &settings), warnings)
}

/// What a single-session command runs on.
#[derive(Debug, Clone, PartialEq)]
enum Pick {
    /// The id or path the user typed.
    Typed(String),
    /// The newest listed session.
    Newest(Box<SessionHeader>),
}

impl Pick {
    fn target(&self) -> Target<'_> {
        match self {
            Self::Typed(id) => Target::Lookup(id),
            Self::Newest(h) => Target::Listed(h),
        }
    }
}

/// The session to run on, and how many listed sessions were unreadable
/// when the newest one had to be found. `remote` lists on the server.
fn resolve_target(
    t: &TargetArgs,
    m: &TargetMessages,
    ctx: &Context,
    roots: &AgentRoots,
    zone: &dyn LocalZone,
    remote: Option<&Remote>,
) -> Result<(Pick, Option<usize>), UserError> {
    let cwd = t.cwd.as_deref().map(|c| {
        let env = &ctx.roots_env;
        select::resolve_cwd(c, &env.home, ctx.cwd.as_deref().unwrap_or(""), env.style)
    });
    match plan_target(
        t.id.as_deref(),
        t.latest,
        cwd.as_deref(),
        t.parent.as_deref(),
        m,
    )? {
        Plan::Picked(id) => Ok((Pick::Typed(id), None)),
        Plan::Newest(cwd) => {
            let (mut headers, unreadable) = if let Some(r) = remote {
                listing_from_json(&r.get_json(&["sessions"], &[])?).ok_or_else(|| r.unexpected())?
            } else {
                let listing = list_sessions(roots, &ListFilter::default());
                let n = listing.unreadable.len();
                (listing.headers, n)
            };
            let picked = select::select_sessions(
                headers.iter().collect(),
                &SelectOptions {
                    since_ms: None,
                    cwd: cwd.clone(),
                    limit: 1,
                },
                zone,
            );
            // TS looks the id up again, which finds its first listed copy.
            let first = picked
                .rows
                .first()
                .and_then(|n| headers.iter().position(|h| h.id == n.id));
            match first {
                Some(at) => Ok((
                    Pick::Newest(Box::new(headers.swap_remove(at))),
                    Some(unreadable),
                )),
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

/// What the `sessions analyze` tables show.
#[derive(Debug, Clone, PartialEq)]
struct Summary {
    session_id: String,
    provider: String,
    context_window: f64,
    peak_context_tokens: f64,
    final_context_tokens: f64,
    turn_count: f64,
    tool_call_count: f64,
    cost: Option<SessionCost>,
    dumb_zone_cross_turn: f64,
    dumb_zone_fraction: f64,
    /// Label and tokens of each budget slice.
    budget: Vec<(String, f64)>,
}

#[expect(clippy::cast_precision_loss, reason = "counts fit f64")]
fn summary_of(a: &AnalyzedSession) -> Summary {
    Summary {
        session_id: a.parsed.session_id.clone(),
        provider: a.parsed.provider.as_str().to_owned(),
        context_window: a.context_window,
        peak_context_tokens: a.peak_context_tokens,
        final_context_tokens: a.final_context_tokens,
        turn_count: a.turn_count as f64,
        tool_call_count: a.tool_call_count as f64,
        cost: a.cost.clone(),
        dumb_zone_cross_turn: a.dumb_zone_cross_turn as f64,
        dumb_zone_fraction: a.dumb_zone_fraction,
        budget: a
            .budget
            .iter()
            .map(|b| (b.label.to_owned(), b.tokens))
            .collect(),
    }
}

/// The same summary from an analysis the server sent.
fn summary_from_json(v: &JsValue) -> Option<Summary> {
    let n = |k: &str| v.get(k).and_then(JsValue::as_num);
    let s = |k: &str| v.get(k).and_then(JsValue::as_str).map(str::to_owned);
    Some(Summary {
        session_id: s("sessionId")?,
        provider: s("provider")?,
        context_window: n("contextWindow")?,
        peak_context_tokens: n("peakContextTokens")?,
        final_context_tokens: n("finalContextTokens")?,
        turn_count: n("turnCount")?,
        tool_call_count: n("toolCallCount")?,
        cost: v.get("cost").and_then(cost_from_json),
        dumb_zone_cross_turn: n("dumbZoneCrossTurn")?,
        dumb_zone_fraction: n("dumbZoneFraction")?,
        budget: v
            .get("budget")?
            .as_arr()?
            .iter()
            .map(|b| {
                Some((
                    b.get("label")?.as_str()?.to_owned(),
                    b.get("tokens")?.as_num()?,
                ))
            })
            .collect::<Option<_>>()?,
    })
}

/// The `sessions analyze` summary and budget tables.
fn render_analysis(a: &Summary, pretty: bool) -> String {
    let fraction = fraction_of(a.peak_context_tokens, a.context_window);
    let cross = if a.dumb_zone_cross_turn < 0.0 {
        "never".to_owned()
    } else {
        num::to_js_string(a.dumb_zone_cross_turn + 1.0)
    };
    let mut rows = vec![
        row("session", a.session_id.clone()),
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
        row("turns", num::to_js_string(a.turn_count)),
        row("tool calls", num::to_js_string(a.tool_call_count)),
    ];
    rows.extend(cost_rows(a.cost.as_ref(), &a.provider));
    rows.push(row("dumb-zone cross turn", cross));
    let rows: Vec<Vec<String>> = rows
        .into_iter()
        .map(|r| r.iter().map(|c| text::to_display(c).into_owned()).collect())
        .collect();
    let summary = render::table(&["FIELD", "VALUE"], &rows, pretty);
    let budget_rows: Vec<Vec<String>> = a
        .budget
        .iter()
        .filter(|(_, t)| *t > 0.0)
        .map(|(label, t)| {
            vec![
                label.clone(),
                tokens(*t),
                if a.peak_context_tokens > 0.0 {
                    percent(t / a.peak_context_tokens)
                } else {
                    "-".to_owned()
                },
            ]
        })
        .collect();
    let budget = render::table(&["BUDGET (AT PEAK)", "TOKENS", "%"], &budget_rows, pretty);
    format!("{summary}\n\n{budget}")
}

/// `sessions analyze`, here or on `remote`.
pub fn run_analyze(
    args: &AnalyzeArgs,
    globals: Globals,
    ctx: &Context,
    remote: Option<&Remote>,
) -> Result<Output, UserError> {
    let (roots, warnings) = match remote {
        Some(_) => (AgentRoots::default(), Vec::new()),
        None => agent_roots(ctx),
    };
    let zone = SystemZone::default();
    let (pick, unreadable) = resolve_target(&args.target, &CLI_TARGET, ctx, &roots, &zone, remote)?;
    let parent = args.target.parent.as_deref();
    let stdout = if let Some(r) = remote {
        let query: Vec<(&str, &str)> = parent.map(|p| ("parent", p)).into_iter().collect();
        let v = r.get_json(&["sessions", pick.target().id(), "analysis"], &query)?;
        if globals.json {
            v.stringify(true)
        } else {
            let s = summary_from_json(&v).ok_or_else(|| r.unexpected())?;
            render_analysis(&s, globals.pretty)
        }
    } else {
        let env = SessionEnv {
            roots: &roots,
            local: &zone,
        };
        let a = analyze_session(env, pick.target(), parent, true)
            .map_err(|e| UserError::Invalid(e.to_string()))?;
        if globals.json {
            analyzed_json(&a)
        } else {
            render_analysis(&summary_of(&a), globals.pretty)
        }
    };
    Ok(Output {
        stdout: Some(stdout),
        write: None,
        warnings,
        unreadable,
        notes: Vec::new(),
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

/// `sessions export`, here or on `remote`.
pub fn run_export(
    args: &ExportArgs,
    globals: Globals,
    ctx: &Context,
    remote: Option<&Remote>,
) -> Result<Output, UserError> {
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
    let (roots, warnings) = match remote {
        Some(_) => (AgentRoots::default(), Vec::new()),
        None => agent_roots(ctx),
    };
    let zone = SystemZone::default();
    let (pick, unreadable) = resolve_target(&args.target, &CLI_TARGET, ctx, &roots, &zone, remote)?;
    let redact = !args.no_redact;
    let parent = args.target.parent.as_deref();
    let session = if let Some(r) = remote {
        let query: Vec<(&str, &str)> = parent
            .map(|p| ("parent", p))
            .into_iter()
            .chain((!redact).then_some(("redact", "false")))
            .collect();
        let v = r.get_json(&["sessions", pick.target().id()], &query)?;
        parsed_from_json(&v).ok_or_else(|| r.unexpected())?
    } else {
        let env = SessionEnv {
            roots: &roots,
            local: &zone,
        };
        get_session(env, pick.target(), parent, redact)
            .map_err(|e| UserError::Invalid(e.to_string()))?
    };
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
            notes: Vec::new(),
        },
        Some(path) => Output {
            stdout: None,
            write: Some((path.to_owned(), rendered)),
            warnings,
            unreadable,
            notes: Vec::new(),
        },
    })
}

#[cfg(test)]
#[path = "single_tests.rs"]
mod tests;
