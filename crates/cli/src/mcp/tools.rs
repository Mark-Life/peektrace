//! The read-only tools `peektrace mcp` serves. Each runs the same code and
//! renderer as its CLI twin, so a tool's text is the compact CLI output. No
//! input has `redact` or `force`: the default redaction holds.

use std::sync::Mutex;

use peektrace_core::Agent;
use peektrace_core::header_cache::HeaderCache;
use peektrace_core::js::value::{JsObject, JsValue};
use peektrace_core::search::result_json;
use peektrace_core::stats::accumulate::DrillTable;
use peektrace_core::stats::service::{DrillPage, ReportRequest, StatsError};
use tracing::{field, info_span};

use super::schema::{Field, Kind, check, input_schema};
use crate::args::TargetArgs;
use crate::ls::{self, Context, Globals, InputNames, LsInput, UserError};
use crate::search::{
    self, SearchInput, no_matches, render_search, truncated_notice, unreadable_notice,
};
use crate::single::{self, TargetMessages};
use crate::stats;

const MCP_NAMES: InputNames = InputNames {
    limit: "limit:",
    since: "since",
};
const MCP_TARGET: TargetMessages = TargetMessages {
    parent_without_id: "parent needs an explicit subagent id.",
    id_and_newest: "Pass either id or latest/cwd, not both.",
    no_target: "Pass id, latest: true, or cwd.",
};
const DRILL_HINT: &str =
    "drill a row with stats_drill: a DRILL cell `--cause K` is {table:\"cause\", key:\"K\"}";
const LS_DEFAULT_LIMIT: f64 = 30.0;
const SEARCH_DEFAULT_LIMIT: f64 = 20.0;

const AGENT: Field = Field {
    name: "agent",
    kind: Kind::Lit {
        title: "Agent to scope to (default all)",
        values: &["claude", "codex", "pi", "opencode", "all"],
    },
    required: false,
};
const FORMAT: Field = Field {
    name: "format",
    kind: Kind::Lit {
        title: "text (default): compact tab-separated tables; json: the same payload as the CLI's --json",
        values: &["text", "json"],
    },
    required: false,
};
const fn text(name: &'static str, desc: &'static str) -> Field {
    Field {
        name,
        kind: Kind::Str { desc },
        required: false,
    }
}
const fn int(name: &'static str, positive: bool, desc: &'static str) -> Field {
    Field {
        name,
        kind: Kind::Int { positive, desc },
        required: false,
    }
}
const DAYS: Field = int("days", true, "History window in days (default 60)");
const ROWS: Field = int("limit", true, "Rows kept per table (default 20)");
const PROJECT: Field = text(
    "project",
    "Project name (Claude/Pi slug, or cwd basename for Codex/OpenCode)",
);
const SINCE: Field = text(
    "since",
    "Only sessions updated since: ISO date (2026-01-31, UTC) or relative 7d, 12h, 2w",
);
const CWD: Field = text(
    "cwd",
    "Only sessions run in this directory or below; `~` is the home dir, relative paths resolve against the server's cwd",
);

/// A tool: its name, description, inputs in schema order and how it runs.
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    fields: &'static [Field],
    run: fn(&Args<'_>, &Call<'_>) -> Result<Answer, UserError>,
}

/// A tool's reply: the main text, and notes sent as their own block so a
/// JSON answer stays parseable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub text: String,
    pub notes: Vec<String>,
}

/// What every call shares: the environment, the list headers kept between
/// calls and the stats lock (one pass at a time, so passes do not race on
/// the cache).
pub struct Shared {
    pub ctx: Context,
    pub headers: HeaderCache,
    pub stats: Mutex<()>,
}

impl Shared {
    pub fn new(ctx: Context) -> Self {
        Self {
            ctx,
            headers: HeaderCache::default(),
            stats: Mutex::new(()),
        }
    }
}

/// One call's view: the context with the clock read now.
struct Call<'a> {
    ctx: Context,
    headers: &'a HeaderCache,
    stats: &'a Mutex<()>,
}

/// Checked tool arguments.
struct Args<'a>(&'a JsObject);

impl Args<'_> {
    fn str(&self, k: &str) -> Option<&str> {
        self.0.get(k).and_then(JsValue::as_str)
    }
    fn num(&self, k: &str) -> Option<f64> {
        self.0.get(k).and_then(JsValue::as_num)
    }
    fn flag(&self, k: &str) -> bool {
        self.0.get(k).is_some_and(JsValue::is_true)
    }
    fn json(&self) -> bool {
        self.str("format") == Some("json")
    }
    /// `agent`, with `all` and absent meaning every agent.
    fn agent(&self) -> Option<Agent> {
        self.str("agent").and_then(Agent::parse)
    }
    fn globals(&self) -> Globals {
        Globals {
            json: self.json(),
            pretty: false,
            read_only: true,
        }
    }
    fn report_request(&self) -> ReportRequest {
        ReportRequest {
            agents: self.agent().map(|a| vec![a]),
            days: self.num("days"),
            force: false,
            limit: self.num("limit"),
        }
    }
}

fn answer(text: Option<String>, notes: Vec<String>) -> Answer {
    Answer {
        text: text.unwrap_or_default(),
        notes,
    }
}

macro_rules! rpc_span {
    ($tag:literal) => {
        info_span!(
            parent: None,
            concat!("rpc.", $tag),
            peektrace.root = true,
            peektrace.kind = "rpc",
            sessions.unreadable = field::Empty,
            error = field::Empty
        )
    };
}

fn traced<T>(
    span: &tracing::Span,
    f: impl FnOnce() -> Result<T, UserError>,
) -> Result<T, UserError> {
    let out = span.in_scope(f);
    if let Err(e) = &out {
        span.record("error", field::display(e));
    }
    out
}

fn sessions_list(a: &Args<'_>, s: &Call<'_>) -> Result<Answer, UserError> {
    let input = LsInput {
        agent: a.agent(),
        project: a.str("project"),
        since: a.str("since"),
        cwd: a.str("cwd"),
        limit: a.num("limit").unwrap_or(LS_DEFAULT_LIMIT),
        headers: Some(s.headers),
    };
    let span = rpc_span!("sessions.listing");
    let out = traced(&span, || {
        ls::list(&input, MCP_NAMES, a.globals(), &s.ctx, None)
    })?;
    span.record("sessions.unreadable", out.unreadable);
    Ok(answer(Some(out.stdout), out.notes))
}

fn sessions_search(a: &Args<'_>, s: &Call<'_>) -> Result<Answer, UserError> {
    let input = SearchInput {
        q: a.str("q").unwrap_or_default().to_owned(),
        agent: a.agent(),
        project: a.str("project"),
        since: a.str("since"),
        limit: a.num("limit").unwrap_or(SEARCH_DEFAULT_LIMIT),
        redact: true,
        headers: Some(s.headers),
    };
    let span = rpc_span!("sessions.search");
    let (run, _) = traced(&span, || search::find(input, MCP_NAMES, &s.ctx))?;
    span.record("sessions.unreadable", run.listing_unreadable);
    let r = &run.result;
    let notes = unreadable_notice(r)
        .into_iter()
        .chain(r.truncated.then(|| truncated_notice(MCP_NAMES)))
        .collect();
    let text = if a.json() {
        result_json(r, true)
    } else if r.sessions.is_empty() {
        no_matches(r)
    } else {
        render_search(&r.sessions, false, None)
    };
    Ok(answer(Some(text), notes))
}

fn sessions_analyze(a: &Args<'_>, s: &Call<'_>) -> Result<Answer, UserError> {
    let target = TargetArgs {
        id: a.str("id").map(str::to_owned),
        latest: a.flag("latest"),
        cwd: a.str("cwd").map(str::to_owned),
        parent: a.str("parent").map(str::to_owned),
    };
    let span = rpc_span!("sessions.analyze");
    let out = traced(&span, || {
        single::analyze(
            &target,
            &MCP_TARGET,
            a.globals(),
            &s.ctx,
            None,
            Some(s.headers),
        )
    })?;
    if let Some(n) = out.unreadable {
        span.record("sessions.unreadable", n);
    }
    Ok(answer(out.stdout, Vec::new()))
}

fn stats_failure(e: StatsError) -> UserError {
    match e {
        StatsError::RowNotFound { table, key } => UserError::Invalid(format!(
            "No {table} row with key \"{key}\". Run stats_report and copy a DRILL cell."
        )),
        StatsError::Scan(reason) => UserError::Invalid(reason),
    }
}

fn stats_report(a: &Args<'_>, s: &Call<'_>) -> Result<Answer, UserError> {
    let req = a.report_request();
    let span = rpc_span!("stats.report");
    let _one = s
        .stats
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let out = traced(&span, || {
        stats::report_output(&req, a.globals(), &s.ctx, DRILL_HINT, false).map_err(stats_failure)
    })?;
    Ok(answer(out.stdout, Vec::new()))
}

fn stats_drill(a: &Args<'_>, s: &Call<'_>) -> Result<Answer, UserError> {
    let table = a
        .str("table")
        .and_then(DrillTable::parse)
        .ok_or_else(|| UserError::Invalid("tools/call needs a table".to_owned()))?;
    let key = a.str("key").unwrap_or_default();
    let page = DrillPage {
        offset: a.num("offset"),
        page_size: a.num("pageSize"),
    };
    let req = a.report_request();
    let span = rpc_span!("stats.drill");
    let _one = s
        .stats
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let out = traced(&span, || {
        stats::drill_output(&req, table, key, page, a.globals(), &s.ctx, false)
            .map_err(stats_failure)
    })?;
    Ok(answer(out.stdout, Vec::new()))
}

/// Every tool, in list order.
pub const TOOLS: [Tool; 5] = [
    Tool {
        name: "sessions_list",
        description: "List local coding-agent sessions (Claude Code, Codex, Pi, OpenCode), newest first: id, agent, project, model, message count, estimated cost, size, update time and title. Use an id with sessions_analyze.",
        fields: &[
            AGENT,
            PROJECT,
            SINCE,
            CWD,
            int(
                "limit",
                false,
                "Max sessions, newest first (default 30, 0 = all)",
            ),
            FORMAT,
        ],
        run: sessions_list,
    },
    Tool {
        name: "sessions_search",
        description: "Find sessions whose transcript contains a phrase (case-insensitive, literal), newest first. Returns each session's id, agent, project, update time, match count and the first matching lines as redacted snippets. Use an id with sessions_analyze.",
        fields: &[
            Field {
                name: "q",
                kind: Kind::Str {
                    desc: "Text to find",
                },
                required: true,
            },
            AGENT,
            PROJECT,
            SINCE,
            int(
                "limit",
                false,
                "Max sessions, newest first (default 20, 0 = all)",
            ),
            FORMAT,
        ],
        run: sessions_search,
    },
    Tool {
        name: "sessions_analyze",
        description: "Context-budget forensics for one session: verdict, peak and final context, turns, tool calls, estimated cost and the token budget at peak. Pass an id (short id works), or latest:true, or a cwd to pick the newest session there.",
        fields: &[
            text("id", "Session id, short id, or transcript path"),
            Field {
                name: "latest",
                kind: Kind::Bool {
                    desc: "Analyze the newest session of any agent",
                },
                required: false,
            },
            CWD,
            text(
                "parent",
                "Parent session id, when id is a subagent transcript (agent-*)",
            ),
            FORMAT,
        ],
        run: sessions_analyze,
    },
    Tool {
        name: "stats_report",
        description: "Cross-session friction report: what to fix first, named causes of retries, what fails and what costs time, with suggested rules for CLAUDE.md / AGENTS.md. Each row has a DRILL cell such as `--cause K`; expand it with stats_drill.",
        fields: &[DAYS, ROWS, AGENT, FORMAT],
        run: stats_report,
    },
    Tool {
        name: "stats_drill",
        description: "The calls behind one stats_report row. A DRILL cell `--cause K` maps to {table:\"cause\", key:\"K\"}; likewise --fail and --time.",
        fields: &[
            Field {
                name: "table",
                kind: Kind::Lit {
                    title: "Table the key comes from",
                    values: &["cause", "fail", "time", "cluster", "wait"],
                },
                required: true,
            },
            Field {
                name: "key",
                kind: Kind::Str {
                    desc: "Key from the DRILL cell, without the --table prefix",
                },
                required: true,
            },
            int("offset", false, "First hit to return (default 0)"),
            int("pageSize", true, "Hits per page (default 200)"),
            DAYS,
            ROWS,
            AGENT,
            FORMAT,
        ],
        run: stats_drill,
    },
];

#[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

impl Tool {
    /// The tool's `tools/list` entry.
    pub fn listing(&self) -> JsValue {
        let annotations = JsObject::from_entries([
            ("readOnlyHint".to_owned(), JsValue::Bool(true)),
            ("openWorldHint".to_owned(), JsValue::Bool(false)),
        ]);
        JsValue::Obj(JsObject::from_entries([
            ("name".to_owned(), JsValue::Str(self.name.to_owned())),
            (
                "description".to_owned(),
                JsValue::Str(self.description.to_owned()),
            ),
            ("inputSchema".to_owned(), input_schema(self.fields)),
            ("annotations".to_owned(), JsValue::Obj(annotations)),
        ]))
    }

    /// Check `args` (`null` reads as `{}`), then run.
    pub fn call(&self, args: &JsValue, shared: &Shared) -> Result<Answer, UserError> {
        let empty = JsValue::Obj(JsObject::default());
        let args = if args.is_null() { &empty } else { args };
        let obj = check(self.fields, args)
            .map_err(|m| UserError::Invalid(format!("Invalid arguments for {}: {m}", self.name)))?;
        let mut ctx = shared.ctx.clone();
        ctx.now_ms = now_ms();
        let call = Call {
            ctx,
            headers: &shared.headers,
            stats: &shared.stats,
        };
        (self.run)(&Args(obj), &call)
    }
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
