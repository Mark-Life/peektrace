//! `peektrace sessions search <q>`: find sessions by text, newest first,
//! redacted by default.

use peektrace_core::js::json_out::Writer;
use peektrace_core::js::{text, units};
use peektrace_core::search::{
    SearchEnv, SearchRequest, SearchResult, SearchSession, search_sessions,
};

use crate::args::SearchArgs;
use crate::ls::{Context, Globals, UserError, parse_agent, parse_bounds, parse_limit};
use crate::render::{self, clip_to_width, printable, short_id};
use crate::single::{Output, agent_roots};
use crate::zone::SystemZone;

const PRETTY_PROJECT_WIDTH: usize = 16;
const DATE_LENGTH: usize = 10;
const HEADERS: [&str; 6] = [
    "SESSION", "AGENT", "PROJECT", "UPDATED", "MATCHES", "SNIPPET",
];

/// `JSON.stringify(result, null, 2)`.
pub fn result_json(r: &SearchResult) -> String {
    let mut w = Writer::new(true);
    w.begin_obj();
    w.key("sessions").begin_arr();
    for s in &r.sessions {
        w.elem();
        session_json(&mut w, s);
    }
    w.end_arr();
    w.key("truncated").bool(r.truncated);
    w.key("scannedSessions").int(count(r.scanned_sessions));
    w.key("unreadable").int(count(r.unreadable));
    w.end_obj();
    w.finish()
}

fn count(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn session_json(w: &mut Writer, s: &SearchSession) {
    w.begin_obj();
    w.key("id").text(&s.id);
    w.key("agent").str(s.agent.as_str());
    w.key("project").text(&s.project);
    if let Some(t) = &s.title {
        w.key("title").text(t);
    }
    w.key("updatedAt").text(&s.updated_at);
    if let Some(src) = &s.source {
        w.key("source").text(src);
    }
    w.key("matchCount").int(count(s.match_count));
    w.key("hits").begin_arr();
    for h in &s.hits {
        w.elem().begin_obj();
        w.key("line").int(count(h.line));
        w.key("snippet").text(&h.snippet);
        if let (Some(a), Some(b)) = (h.match_start, h.match_end) {
            w.key("matchStart").int(count(a));
            w.key("matchEnd").int(count(b));
        }
        w.end_obj();
    }
    w.end_arr();
    w.end_obj();
}

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

/// The search text: the words joined by one space, not blank.
fn query(words: &[String]) -> Result<String, UserError> {
    let q = words.join(" ");
    if text::trim(&q).is_empty() {
        return Err(UserError::Invalid(
            "Give a non-empty search text.".to_owned(),
        ));
    }
    Ok(q)
}

/// The listing's unreadable count, which the root span records, and the
/// command output.
pub fn run(args: &SearchArgs, globals: Globals, ctx: &Context) -> Result<Output, UserError> {
    let limit = parse_limit(&args.limit)?;
    let agent = parse_agent(args.agent.as_deref())?;
    let q = query(&args.q)?;
    let (limit, since_ms) = parse_bounds(limit, args.since.as_deref(), ctx.now_ms)?;
    let (roots, warnings) = agent_roots(ctx);
    let home = (!ctx.roots_env.home.is_empty()).then_some(ctx.roots_env.home.as_str());
    let zone = SystemZone::default();
    let run = search_sessions(
        SearchEnv {
            roots: &roots,
            home,
            local: &zone,
        },
        &SearchRequest {
            q,
            agent,
            project: args.project.clone(),
            since_ms,
            limit,
            redact: !args.no_redact,
        },
    );
    let r = &run.result;
    let unreadable = (r.unreadable > 0).then(|| {
        let noun = if r.unreadable == 1 {
            "session"
        } else {
            "sessions"
        };
        format!("{} {noun} could not be read", r.unreadable)
    });
    let (stdout, tail) = if globals.json {
        (Some(result_json(r)), None)
    } else if r.sessions.is_empty() {
        (
            None,
            Some(format!("no matches in {} sessions", r.scanned_sessions)),
        )
    } else {
        let width = globals.pretty.then(stdout_columns).flatten();
        (
            Some(render_search(&r.sessions, globals.pretty, width)),
            r.truncated
                .then(|| "more sessions match, use --limit 0 to show all".to_owned()),
        )
    };
    Ok(Output {
        stdout,
        write: None,
        warnings,
        unreadable: Some(run.listing_unreadable),
        notes: unreadable.into_iter().chain(tail).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use peektrace_core::Agent;
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
        let j = result_json(&r);
        assert!(j.contains("\"matchStart\": 4"), "{j}");
        assert!(
            j.ends_with("\"truncated\": false,\n  \"scannedSessions\": 3,\n  \"unreadable\": 0\n}")
        );
    }

    #[test]
    fn blank_query_is_refused() {
        assert!(query(&["  ".to_owned()]).is_err());
        assert_eq!(
            query(&["a".to_owned(), "b".to_owned()]),
            Ok("a b".to_owned())
        );
    }
}
