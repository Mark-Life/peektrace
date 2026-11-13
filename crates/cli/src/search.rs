//! `peektrace sessions search <q>`: find sessions by text, newest first,
//! redacted by default.

use peektrace_core::Agent;
use peektrace_core::header_cache::HeaderCache;
use peektrace_core::js::{text, units};
use peektrace_core::search::{
    SearchEnv, SearchRequest, SearchResult, SearchRun, SearchSession, result_json, search_sessions,
};

use crate::args::SearchArgs;
use crate::ls::{
    CLI_NAMES, Context, Globals, InputNames, UserError, parse_agent, parse_bounds, parse_limit,
};
use crate::render::{self, clip_to_width, printable, short_id};
use crate::single::{Output, agent_roots};
use peektrace_core::js::system_zone::SystemZone;

const PRETTY_PROJECT_WIDTH: usize = 16;
const DATE_LENGTH: usize = 10;
const HEADERS: [&str; 6] = [
    "SESSION", "AGENT", "PROJECT", "UPDATED", "MATCHES", "SNIPPET",
];

/// One row per hit; the session columns fill only the first. The pretty
/// table shortens project and date so the fixed columns fit a narrow
/// terminal.
fn session_rows(s: &SearchSession, pretty: bool) -> Vec<Vec<String>> {
    s.hits
        .iter()
        .enumerate()
        .map(|(i, hit)| {
            let mut row = if i == 0 {
                let project = printable(&s.project);
                vec![
                    printable(short_id(&s.id)),
                    s.agent.as_str().to_owned(),
                    if pretty {
                        clip_to_width(&project, PRETTY_PROJECT_WIDTH)
                    } else {
                        project
                    },
                    if pretty {
                        printable(&units::slice(&s.updated_at, 0, DATE_LENGTH))
                    } else {
                        printable(&s.updated_at)
                    },
                    s.match_count.to_string(),
                ]
            } else {
                vec![String::new(); 5]
            };
            row.push(format!("L{}  {}", hit.line, printable(&hit.snippet)));
            row
        })
        .collect()
}

/// The hits table.
pub fn render_search(sessions: &[SearchSession], pretty: bool, max_width: Option<usize>) -> String {
    let rows: Vec<Vec<String>> = sessions
        .iter()
        .flat_map(|s| session_rows(s, pretty))
        .collect();
    render::fit_table(&HEADERS, &rows, pretty, max_width)
}

/// Terminal columns of stdout, when it is a terminal.
fn stdout_columns() -> Option<usize> {
    terminal_size::terminal_size_of(std::io::stdout())
        .map(|(w, _)| usize::from(w.0))
        .filter(|&w| w > 0)
}

/// The search text, not blank.
fn query(q: String) -> Result<String, UserError> {
    if text::trim(&q).is_empty() {
        return Err(UserError::Invalid(
            "Give a non-empty search text.".to_owned(),
        ));
    }
    Ok(q)
}

/// Typed `sessions search` inputs, shared by the CLI and the MCP tool.
#[derive(Debug, Clone)]
pub struct SearchInput<'a> {
    pub q: String,
    pub agent: Option<Agent>,
    pub project: Option<&'a str>,
    pub since: Option<&'a str>,
    pub limit: f64,
    pub redact: bool,
    /// Headers kept between calls by a long-lived process.
    pub headers: Option<&'a HeaderCache>,
}

/// Run `sessions search` against `ctx`. `unreadable` is the listing's
/// count, which the root span records.
pub fn run(args: &SearchArgs, globals: Globals, ctx: &Context) -> Result<Output, UserError> {
    let limit = parse_limit(&args.limit)?;
    let agent = parse_agent(args.agent.as_deref())?;
    let input = SearchInput {
        q: args.q.join(" "),
        agent,
        project: args.project.as_deref(),
        since: args.since.as_deref(),
        limit,
        redact: !args.no_redact,
        headers: None,
    };
    let (run, warnings) = find(input, CLI_NAMES, ctx)?;
    let r = &run.result;
    let (stdout, tail) = if globals.json {
        (Some(result_json(r, true)), None)
    } else if r.sessions.is_empty() {
        (None, Some(no_matches(r)))
    } else {
        let width = globals.pretty.then(stdout_columns).flatten();
        (
            Some(render_search(&r.sessions, globals.pretty, width)),
            r.truncated.then(|| truncated_notice(CLI_NAMES)),
        )
    };
    Ok(Output {
        stdout,
        write: None,
        warnings,
        unreadable: Some(run.listing_unreadable),
        notes: unreadable_notice(r).into_iter().chain(tail).collect(),
    })
}

pub fn no_matches(r: &SearchResult) -> String {
    format!("no matches in {} sessions", r.scanned_sessions)
}

pub fn truncated_notice(names: InputNames) -> String {
    format!("more sessions match, use {} 0 to show all", names.limit)
}

pub fn unreadable_notice(r: &SearchResult) -> Option<String> {
    (r.unreadable > 0).then(|| {
        let noun = if r.unreadable == 1 {
            "session"
        } else {
            "sessions"
        };
        format!("{} {noun} could not be read", r.unreadable)
    })
}

/// The search for typed inputs, plus the settings warnings; `names` words
/// the messages.
pub fn find(
    input: SearchInput<'_>,
    names: InputNames,
    ctx: &Context,
) -> Result<(SearchRun, Vec<String>), UserError> {
    let q = query(input.q)?;
    let (limit, since_ms) = parse_bounds(input.limit, input.since, ctx.now_ms, names)?;
    let (roots, warnings) = agent_roots(ctx);
    let home = (!ctx.roots_env.home.is_empty()).then_some(ctx.roots_env.home.as_str());
    let zone = SystemZone::default();
    let run = search_sessions(
        SearchEnv {
            roots: &roots,
            home,
            local: &zone,
            headers: input.headers,
        },
        &SearchRequest {
            q,
            agent: input.agent,
            project: input.project.map(str::to_owned),
            since_ms,
            limit,
            redact: input.redact,
        },
    );
    Ok((run, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use peektrace_core::search::SearchHit;

    fn session() -> SearchSession {
        SearchSession {
            id: "abc-def".to_owned(),
            agent: Agent::Claude,
            project: "sessions-opencode".to_owned(),
            title: None,
            updated_at: "2026-11-12T03:32:31.236Z".to_owned(),
            source: None,
            match_count: 2,
            hits: vec![
                SearchHit {
                    line: 3,
                    snippet: "hey there".to_owned(),
                    match_start: Some(4),
                    match_end: Some(9),
                },
                SearchHit {
                    line: 5,
                    snippet: "again".to_owned(),
                    match_start: None,
                    match_end: None,
                },
            ],
        }
    }

    #[test]
    fn compact_rows() {
        assert_eq!(
            render_search(&[session()], false, Some(10)),
            "session\tagent\tproject\tupdated\tmatches\tsnippet\n\
             abc\tclaude\tsessions-opencode\t2026-11-12T03:32:31.236Z\t2\tL3  hey there\n\
             \t\t\t\t\tL5  again"
        );
    }

    #[test]
    fn pretty_rows_shorten_project_and_date() {
        let t = render_search(&[session()], true, None);
        assert!(t.contains("sessions-openco…  2026-11-12  2"), "{t}");
    }

    #[test]
    fn json_keys() {
        let r = SearchResult {
            sessions: vec![session()],
            truncated: false,
            scanned_sessions: 3,
            unreadable: 0,
        };
        let j = result_json(&r, true);
        assert!(j.contains("\"matchStart\": 4"), "{j}");
        assert!(
            j.ends_with("\"truncated\": false,\n  \"scannedSessions\": 3,\n  \"unreadable\": 0\n}")
        );
    }

    #[test]
    fn blank_query_is_refused() {
        assert!(query("  ".to_owned()).is_err());
        assert_eq!(query("a b".to_owned()), Ok("a b".to_owned()));
    }
}
