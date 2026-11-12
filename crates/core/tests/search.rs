//! Session search over a throwaway corpus of synthetic sessions from every
//! agent (port of `packages/core/test/sessions/search.test.ts`).

mod common;

use std::sync::LazyLock;

use common::{TempDir, path_str, write_good_db};
use peektrace_core::export::home::HomeCollapser;
use peektrace_core::js::time::Utc;
use peektrace_core::roots::{RootsEnv, Settings};
use peektrace_core::search::{
    SearchEnv, SearchRequest, SearchResult, redact_display, search_sessions,
};
use peektrace_core::{Agent, AgentRoots};
use serde_json::{Value, json};

const CWD: &str = "/home/demo/work";
const RAW_TITLE: &str = "Fix parser in /home/demo/tools/app";
const BRANCH: &str = "feature/zebra-branch";
const FAKE_KEY: &str = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";
const FILLER: usize = 30;
const OC: &str = "ses_search1";

fn id(c: char) -> String {
    let r = |n| c.to_string().repeat(n);
    format!("{}-{}-4{}-8{}-{}", r(8), r(4), r(3), r(3), r(12))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "call sites build the value inline"
)]
fn claude_line(kind: &str, at: &str, content: Value) -> Value {
    json!({"type":kind,"cwd":CWD,"gitBranch":BRANCH,"version":"2.0.0","timestamp":at,
        "sessionId":"s","uuid":format!("u-{at}"),"message":{"role":kind,"content":content}})
}

fn lines(rows: &[Value]) -> String {
    rows.iter()
        .fold(String::new(), |out, r| out + &r.to_string() + "\n")
}

struct Corpus {
    dir: TempDir,
    roots: AgentRoots,
}

fn build() -> Corpus {
    let dir = TempDir::new("search-corpus");
    let proj = "claude";
    dir.write(
        &format!("{proj}/-home-demo-work/{}.jsonl", id('a')),
        &lines(&[
            claude_line(
                "user",
                "2026-05-03T10:00:00.000Z",
                json!("Fix the Quokka parser"),
            ),
            claude_line(
                "assistant",
                "2026-05-03T10:00:01.000Z",
                json!([{"type":"text","text":"Looking at the quokka parser now"}]),
            ),
            claude_line(
                "user",
                "2026-05-03T10:00:02.000Z",
                json!(format!("key {FAKE_KEY} quokka")),
            ),
            claude_line(
                "user",
                "2026-05-03T10:00:03.000Z",
                json!("quokka again\nsecond \"quote\" line"),
            ),
            claude_line("user", "2026-05-03T10:00:04.000Z", json!("another quokka")),
            json!({"type":"summary","summary":"Fix the Quokka parser","leafUuid":"x"}),
            json!({"type":"ai-title","aiTitle":RAW_TITLE}),
        ]),
    );
    dir.write(
        &format!("{proj}/-home-demo-work/{}.jsonl", id('b')),
        &lines(&[claude_line(
            "user",
            "2026-04-03T10:00:00.000Z",
            json!("quokka in the middle"),
        )]),
    );
    dir.write(
        &format!("{proj}/-home-demo-other/{}.jsonl", id('c')),
        &lines(&[claude_line(
            "user",
            "2026-01-03T10:00:00.000Z",
            json!("quokka long ago"),
        )]),
    );
    let codex = id('d');
    dir.write(
        &format!("codex/2026/05/01/rollout-2026-05-01T10-00-00-{codex}.jsonl"),
        &lines(&[
            json!({"timestamp":"2026-05-01T10:00:00.000Z","type":"session_meta",
                "payload":{"id":codex,"timestamp":"2026-05-01T10:00:00.000Z","cwd":"/home/demo/cx"}}),
            json!({"timestamp":"2026-05-01T10:00:01.000Z","type":"response_item",
                "payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"find the wombat"}]}}),
            json!({"timestamp":"2026-05-01T10:00:01.000Z","type":"event_msg",
                "payload":{"type":"user_message","message":"find the wombat"}}),
        ]),
    );
    let pi = id('e');
    dir.write(
        &format!("pi/--home-demo-pi--/2026-02-01T10-00-00-000Z_{pi}.jsonl"),
        &lines(&[
            json!({"type":"session","version":3,"id":pi,"timestamp":"2026-02-01T10:00:00.000Z","cwd":"/home/demo/pi"}),
            json!({"type":"message","id":"a1","parentId":null,"timestamp":"2026-02-01T10:00:01.000Z",
                "message":{"role":"user","content":[{"type":"text","text":"a pi quokka"}]}}),
        ]),
    );
    for i in 0..FILLER {
        let n = format!("{i:02}");
        let day = if n == "00" { "01" } else { "02" };
        dir.write(
            &format!("{proj}/-home-demo-filler/{}-{n}.jsonl", id('f')),
            &lines(&[claude_line(
                "user",
                &format!("2025-01-{day}T10:00:00.000Z"),
                json!(format!("filler {n}")),
            )]),
        );
    }
    let oc = dir.mkdir("opencode");
    write_good_db(
        &std::path::Path::new(&oc).join("opencode.db"),
        OC,
        "/home/demo/oc",
    );
    let env = RootsEnv {
        home: dir.join("home"),
        claude_projects: Some(dir.join(proj)),
        codex_sessions: Some(dir.join("codex")),
        pi_sessions: Some(dir.join("pi")),
        opencode_data: Some(oc),
        ..RootsEnv::default()
    };
    let roots = AgentRoots::resolve(&env, &Settings::default());
    Corpus { dir, roots }
}

static CORPUS: LazyLock<Corpus> = LazyLock::new(build);

fn request(q: &str) -> SearchRequest {
    SearchRequest {
        q: q.to_owned(),
        limit: 20,
        redact: true,
        ..SearchRequest::default()
    }
}

fn search_in(roots: &AgentRoots, req: &SearchRequest) -> SearchResult {
    search_sessions(
        SearchEnv {
            roots,
            home: None,
            local: &Utc,
        },
        req,
    )
    .result
}

fn search(req: &SearchRequest) -> SearchResult {
    search_in(&CORPUS.roots, req)
}

fn ids(r: &SearchResult) -> Vec<String> {
    r.sessions.iter().map(|s| s.id.clone()).collect()
}

fn slice16(s: &str, a: usize, b: usize) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    String::from_utf16_lossy(&u[a..b])
}

#[test]
fn returns_matching_sessions_newest_first_across_agents() {
    let r = search(&request("quokka"));
    assert_eq!(ids(&r), [id('a'), id('b'), id('e'), id('c')]);
    assert!(!r.truncated);
    assert_eq!(r.scanned_sessions, 6 + FILLER);
}

#[test]
fn is_case_insensitive_and_reports_the_line_and_match_range() {
    let r = search(&request("QUOKKA PARSER"));
    let s = &r.sessions[0];
    assert_eq!(s.id, id('a'));
    let hit = &s.hits[0];
    assert_eq!(hit.line, 1);
    assert_eq!(hit.snippet, "Fix the Quokka parser");
    let (a, b) = (hit.match_start.expect("start"), hit.match_end.expect("end"));
    assert_eq!(slice16(&hit.snippet, a, b), "Quokka parser");
}

#[test]
fn caps_hits_per_session_but_counts_every_matching_line() {
    let s = &search(&request("quokka")).sessions[0];
    assert_eq!(s.hits.len(), 3);
    assert_eq!(s.match_count, 6);
}

#[test]
fn a_query_equal_to_metadata_values_matches_nothing() {
    for q in [CWD, BRANCH, "2.0.0", "user", "2026-05-03T10"] {
        assert!(search(&request(q)).sessions.is_empty(), "{q}");
    }
}

#[test]
fn a_query_with_a_double_quote_matches_the_escaped_json_text() {
    let r = search(&request("second \"quote\""));
    assert_eq!(ids(&r), [id('a')]);
    assert!(r.sessions[0].hits[0].snippet.contains("second \"quote\""));
}

#[test]
fn codex_text_written_twice_yields_one_hit() {
    let r = search(&request("wombat"));
    assert_eq!(ids(&r), [id('d')]);
    assert_eq!(r.sessions[0].hits.len(), 1);
}

#[test]
fn reads_opencode_sessions_through_the_registry() {
    let r = search(&request(&format!("hi from {OC}")));
    assert_eq!(ids(&r), [OC]);
    assert_eq!(r.sessions[0].agent, Agent::Opencode);
}

#[test]
fn filters_by_agent_project_and_since() {
    let with = |f: &dyn Fn(&mut SearchRequest)| {
        let mut req = request("quokka");
        f(&mut req);
        ids(&search(&req))
    };
    assert_eq!(with(&|r| r.agent = Some(Agent::Pi)), [id('e')]);
    assert_eq!(
        with(&|r| r.project = Some("-home-demo-other".to_owned())),
        [id('c')]
    );
    assert_eq!(
        with(&|r| r.since_ms = Some(1_775_001_600_000.0)),
        [id('a'), id('b')]
    );
}

#[test]
fn limit_counts_sessions_and_truncated_is_true_only_when_more_matched() {
    let at = |limit| {
        search(&SearchRequest {
            limit,
            ..request("quokka")
        })
    };
    let some = at(2);
    assert_eq!(ids(&some), [id('a'), id('b')]);
    assert!(some.truncated);
    let exact = at(4);
    assert_eq!(exact.sessions.len(), 4);
    assert!(!exact.truncated);
    assert_eq!(at(0).sessions.len(), 4);
}

#[test]
fn stops_scanning_once_the_limit_is_met() {
    let r = search(&SearchRequest {
        limit: 1,
        ..request("quokka")
    });
    assert!(r.scanned_sessions < 6 + FILLER);
    // Up to the second match, the third session newest first.
    assert_eq!(r.scanned_sessions, 3);
}

#[test]
fn redacts_secrets_by_default_and_keeps_them_on_request() {
    let on = search(&SearchRequest {
        limit: 1,
        ..request("quokka")
    });
    let text = format!("{on:?}");
    assert!(!text.contains(FAKE_KEY));
    assert!(text.contains("[REDACTED"));
    let off = search(&SearchRequest {
        redact: false,
        ..request(FAKE_KEY)
    });
    assert!(off.sessions[0].hits[0].snippet.contains(FAKE_KEY));
}

#[test]
fn redacts_project_and_title_and_leaves_them_raw_on_request() {
    let home = HomeCollapser::new(None);
    let on = search(&SearchRequest {
        agent: Some(Agent::Claude),
        ..request("quokka")
    });
    assert!(!on.sessions[0].project.contains("/home/demo"));
    assert_eq!(
        on.sessions[0].project,
        redact_display("-home-demo-work", &home)
    );
    assert_ne!(redact_display(RAW_TITLE, &home), RAW_TITLE);
    assert_eq!(
        on.sessions[0].title.as_deref(),
        Some(redact_display(RAW_TITLE, &home).as_str())
    );
    let off = search(&SearchRequest {
        agent: Some(Agent::Claude),
        redact: false,
        ..request("quokka")
    });
    assert_eq!(off.sessions[0].project, "-home-demo-work");
    assert_eq!(off.sessions[0].title.as_deref(), Some(RAW_TITLE));
}

#[test]
fn a_redacted_project_name_filters_like_the_raw_one() {
    let shown = redact_display("-home-demo-other", &HomeCollapser::new(None));
    assert_ne!(shown, "-home-demo-other");
    let r = search(&SearchRequest {
        project: Some(shown),
        ..request("quokka")
    });
    assert_eq!(ids(&r), [id('c')]);
}

#[test]
fn metadata_such_as_a_git_branch_is_not_searched() {
    assert_eq!(search(&request("zebra-branch")).sessions, []);
}

#[test]
fn a_blank_query_does_not_scan_everything() {
    assert_eq!(search(&request("   ")), SearchResult::default());
}

#[cfg(unix)]
#[test]
fn counts_a_session_it_cannot_read() {
    use std::os::unix::fs::PermissionsExt as _;
    let c = build();
    let file = c.dir.write(
        &format!("claude/-home-demo-locked/{}.jsonl", id('9')),
        &lines(&[claude_line(
            "user",
            "2026-05-09T10:00:00.000Z",
            json!("quokka"),
        )]),
    );
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    if std::fs::read(&file).is_ok() {
        return;
    }
    let r = search_in(&c.roots, &request("quokka"));
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    assert_eq!(r.unreadable, 1);
    assert!(!ids(&r).contains(&id('9')));
}

#[test]
fn no_match_gives_an_empty_result() {
    let r = search(&request("nothing-like-this"));
    assert_eq!(r.sessions, []);
    assert!(!r.truncated);
    assert_eq!(r.unreadable, 0);
    let _ = path_str(CORPUS.dir.path());
}
