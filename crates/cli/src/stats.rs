//! `peektrace stats` and its subcommands: validate flags, run core, print.

use peektrace_core::js::num;
use peektrace_core::js::text;
use peektrace_core::listing::session_files;
use peektrace_core::roots::RootsEnv;
use peektrace_core::stats::accumulate::DrillTable;
use peektrace_core::stats::cache::StatsCache;
use peektrace_core::stats::detectors::usage::UsageBy;
use peektrace_core::stats::icu_zones::icu_zone_name;
use peektrace_core::stats::pass::{parse_for, scope_of};
use peektrace_core::stats::prompts::{PromptTarget, stats_prompt};
use peektrace_core::stats::service::{ReportRequest, StatsEnv, StatsError, drill, refresh, report};
use peektrace_core::stats::usage_service::{MAX_USAGE_DAYS, UsageRequest, usage, zone_of};
use peektrace_core::{Agent, AgentRoots};

use crate::args::{StatsCommon, StatsDrillArgs, StatsPromptArgs, StatsRefreshArgs, StatsUsageArgs};
use crate::ls::{Context, Globals, UserError};
use crate::render::table;
use crate::single::{Output, agent_roots};
use crate::stats_progress::{self, Progress};
use crate::stats_render::{render_drill, render_report, render_usage};
use crate::zone::SystemZone;
use std::io::{IsTerminal as _, Write as _};

/// `Options.integer`: `Number(s)` that is a safe integer, blank rejected.
fn parse_int(raw: &str) -> Result<f64, UserError> {
    let n = num::parse_number(raw);
    if text::trim(raw).is_empty() || !num::is_safe_integer(n) {
        return Err(UserError::Validation(format!("'{raw}' is not a integer")));
    }
    Ok(n)
}

fn parse_opt_int(raw: Option<&str>) -> Result<Option<f64>, UserError> {
    raw.map(parse_int).transpose()
}

/// `--agent`: absent or `all` is every agent stats can extract; any other
/// agent fails.
fn parse_agent(raw: Option<&str>) -> Result<Option<Vec<Agent>>, UserError> {
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
    if parse_for(agent).is_none() {
        let supported: Vec<&str> = scope_of(None).into_iter().map(Agent::as_str).collect();
        return Err(UserError::Invalid(format!(
            "Agent \"{value}\" is not supported yet. Supported: {}.",
            supported.join(", ")
        )));
    }
    Ok(Some(vec![agent]))
}

/// Names as prose: `a`, `a and b`, `a, b and c`.
fn and_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Stderr note naming the agents stats cannot extract yet whose roots hold
/// sessions; only when no `--agent` narrows the scan.
fn not_scanned_note(roots: &AgentRoots, agents: Option<&[Agent]>) -> Option<String> {
    if agents.is_some() {
        return None;
    }
    let names: Vec<&str> = Agent::ALL
        .into_iter()
        .filter(|a| parse_for(*a).is_none() && !session_files(*a, roots, None).is_empty())
        .map(Agent::as_str)
        .collect();
    (!names.is_empty()).then(|| {
        format!(
            "{} sessions are not scanned: not supported yet",
            and_list(&names)
        )
    })
}

fn cache_root(env: &RootsEnv) -> String {
    let dir = env
        .peektrace_dir
        .clone()
        .unwrap_or_else(|| env.style.join(&[&env.home, ".peektrace"]));
    env.style.join(&[&dir, "stats-rs"])
}

struct Prepared {
    roots: AgentRoots,
    warnings: Vec<String>,
    notes: Vec<String>,
}

fn prepare(ctx: &Context, agents: Option<&[Agent]>) -> Prepared {
    let (roots, warnings) = agent_roots(ctx);
    let notes = not_scanned_note(&roots, agents).into_iter().collect();
    Prepared {
        roots,
        warnings,
        notes,
    }
}

fn env_of<'a>(
    ctx: &Context,
    p: &'a Prepared,
    zone: &'a SystemZone,
    progress: Option<&'a dyn Fn(usize, usize)>,
) -> StatsEnv<'a> {
    let e = &ctx.roots_env;
    StatsEnv {
        roots: &p.roots,
        cache: StatsCache::new(cache_root(e)),
        local: zone,
        now_ms: ctx.now_ms,
        progress,
    }
}

/// Run `f` with a progress line on a capable TTY stderr, cleared when it returns.
fn with_progress<T>(f: impl FnOnce(Option<&dyn Fn(usize, usize)>) -> T) -> T {
    let term = std::env::var("TERM").ok();
    if !stats_progress::wanted(std::io::stderr().is_terminal(), term.as_deref()) {
        return f(None);
    }
    let start = std::time::Instant::now();
    let sink = Progress::new(
        move || start.elapsed().as_secs_f64() * 1000.0,
        |s: &str| {
            let _ = std::io::stderr().write_all(s.as_bytes());
        },
    );
    let on_file = |done: usize, total: usize| sink.on_file(done, total);
    let out = f(Some(&on_file));
    sink.clear();
    out
}

fn scan_error(e: StatsError) -> UserError {
    match e {
        StatsError::Scan(reason) => UserError::Invalid(reason),
        other @ StatsError::RowNotFound { .. } => UserError::Invalid(other.to_string()),
    }
}

fn output(p: Prepared, stdout: &str) -> Output {
    Output {
        stdout: Some(text::to_display(stdout).into_owned()),
        write: None,
        warnings: p.warnings,
        unreadable: None,
        notes: p.notes,
    }
}

fn request(common: &StatsCommon) -> Result<ReportRequest, UserError> {
    let days = parse_opt_int(common.days.as_deref())?;
    let agents = parse_agent(common.agent.as_deref())?;
    let limit = parse_opt_int(common.limit.as_deref())?;
    Ok(ReportRequest {
        agents,
        days,
        force: false,
        limit,
    })
}

/// `stats`: the ranked tables.
pub fn run_report(common: &StatsCommon, g: Globals, ctx: &Context) -> Result<Output, UserError> {
    let req = request(common)?;
    let p = prepare(ctx, req.agents.as_deref());
    let zone = SystemZone::default();
    let r = with_progress(|pr| report(&env_of(ctx, &p, &zone, pr), &req)).map_err(scan_error)?;
    let out = if g.json {
        r.to_json(true)
    } else {
        render_report(&r, g.pretty)
    };
    Ok(output(p, &out))
}

/// `stats drill`: the calls behind one row.
pub fn run_drill(a: &StatsDrillArgs, g: Globals, ctx: &Context) -> Result<Output, UserError> {
    let picked: Vec<(DrillTable, &str)> = [
        (DrillTable::Cause, &a.cause),
        (DrillTable::Fail, &a.fail),
        (DrillTable::Time, &a.time),
        (DrillTable::Cluster, &a.cluster),
        (DrillTable::Wait, &a.wait),
    ]
    .into_iter()
    .filter_map(|(t, k)| k.as_deref().map(|k| (t, k)))
    .collect();
    let [(table_, key)] = picked[..] else {
        return Err(UserError::Invalid(
            "Pass exactly one of --cause, --fail, --time, --cluster or --wait with a key from the table.".to_owned(),
        ));
    };
    let req = request(&a.common)?;
    let p = prepare(ctx, req.agents.as_deref());
    let zone = SystemZone::default();
    let d = with_progress(|pr| drill(&env_of(ctx, &p, &zone, pr), &req, table_, key))
        .map_err(scan_error)?;
    let out = if g.json {
        d.to_json(true)
    } else {
        render_drill(&d, g.pretty)
    };
    Ok(output(p, &out))
}

/// `stats prompt`: the investigation prompt for the report or one row.
pub fn run_prompt(a: &StatsPromptArgs, ctx: &Context) -> Result<Output, UserError> {
    let picked: Vec<PromptTarget> = [
        a.cause.clone().map(PromptTarget::Cause),
        a.fail.clone().map(PromptTarget::Fail),
        a.time.clone().map(PromptTarget::Time),
    ]
    .into_iter()
    .flatten()
    .collect();
    if picked.len() > 1 {
        return Err(UserError::Invalid(
            "Pass at most one of --cause, --fail or --time.".to_owned(),
        ));
    }
    let target = picked.into_iter().next().unwrap_or(PromptTarget::Report);
    let req = request(&a.common)?;
    let p = prepare(ctx, req.agents.as_deref());
    let zone = SystemZone::default();
    let r = with_progress(|pr| report(&env_of(ctx, &p, &zone, pr), &req)).map_err(scan_error)?;
    let agent = req.agents.as_ref().and_then(|v| v.first().copied());
    let Some(text) = stats_prompt(&r, agent, &target) else {
        let (table_, key) = match &target {
            PromptTarget::Cause(k) => ("cause", k.as_str()),
            PromptTarget::Fail(k) => ("fail", k.as_str()),
            PromptTarget::Time(k) => ("time", k.as_str()),
            PromptTarget::Report => ("report", ""),
        };
        return Err(UserError::Invalid(format!(
            "No {table_} row \"{key}\" in this window's top rows. List the keys with: peektrace stats; for a row ranked past 20, pass the --limit that lists it"
        )));
    };
    Ok(output(p, &text))
}

/// `stats refresh`: re-extract the cache.
#[expect(
    clippy::many_single_char_names,
    reason = "short names for args, globals and summary"
)]
pub fn run_refresh(a: &StatsRefreshArgs, g: Globals, ctx: &Context) -> Result<Output, UserError> {
    let agents = parse_agent(a.agent.as_deref())?;
    let p = prepare(ctx, agents.as_deref());
    let zone = SystemZone::default();
    let s = with_progress(|pr| refresh(&env_of(ctx, &p, &zone, pr), agents.as_deref(), a.force))
        .map_err(scan_error)?;
    let out = if g.json {
        let mut w = peektrace_core::js::json_out::Writer::new(true);
        w.begin_obj();
        #[expect(clippy::cast_possible_wrap)]
        {
            w.key("transcripts").int(s.transcripts as i64);
            w.key("extracted").int(s.extracted as i64);
            w.key("cached").int(s.cached as i64);
            w.key("skipped").texts(s.skipped.iter().map(String::as_str));
            w.key("facts").int(s.facts as i64);
        }
        w.end_obj();
        w.finish()
    } else {
        let rows: Vec<Vec<String>> = [
            ("transcripts", s.transcripts),
            ("extracted", s.extracted),
            ("from cache", s.cached),
            ("facts", s.facts),
            ("skipped", s.skipped.len()),
        ]
        .iter()
        .map(|(k, v)| vec![(*k).to_owned(), v.to_string()])
        .collect();
        table(&["FIELD", "VALUE"], &rows, g.pretty)
    };
    Ok(output(p, &out))
}

/// The machine's zone name, as `Intl.DateTimeFormat().resolvedOptions()` gives it.
fn machine_zone() -> String {
    std::env::var("TZ")
        .ok()
        .filter(|tz| !tz.is_empty() && zone_of(tz).is_some())
        .or_else(|| jiff::tz::TimeZone::system().iana_name().map(str::to_owned))
        .map_or_else(|| "UTC".to_owned(), |name| icu_zone_name(&name).to_owned())
}

/// `stats usage`: tokens and cost per day, month or block.
pub fn run_usage(a: &StatsUsageArgs, g: Globals, ctx: &Context) -> Result<Output, UserError> {
    let by = match a.by.as_deref() {
        None => UsageBy::Day,
        Some(raw) => UsageBy::parse(raw).ok_or_else(|| {
            UserError::Validation(
                "Expected one of the following cases: day, month, block".to_owned(),
            )
        })?,
    };
    let days = parse_opt_int(a.days.as_deref())?;
    let agents = parse_agent(a.agent.as_deref())?;
    let tz = match &a.tz {
        None => machine_zone(),
        Some(t) if zone_of(t).is_some() => t.clone(),
        Some(t) => {
            return Err(UserError::Invalid(format!(
                "Unknown time zone \"{t}\". Pass an IANA name such as UTC or Europe/Berlin."
            )));
        }
    };
    #[expect(clippy::cast_possible_truncation)]
    let days = match days {
        Some(d) if !(1.0..=36_500.0).contains(&d) => {
            return Err(UserError::Invalid(format!(
                "--days must be from 1 to {MAX_USAGE_DAYS}, got {}.",
                num::to_js_string(d)
            )));
        }
        other => other.map(|d| d as i64),
    };
    let p = prepare(ctx, agents.as_deref());
    let zone = SystemZone::default();
    let req = UsageRequest {
        agents,
        by,
        days,
        force: a.force,
        tz: Some(tz),
    };
    let u = with_progress(|pr| usage(&env_of(ctx, &p, &zone, pr), &req)).map_err(scan_error)?;
    let out = if g.json {
        u.to_json(true)
    } else {
        render_usage(&u, g.pretty)
    };
    Ok(output(p, &out))
}
