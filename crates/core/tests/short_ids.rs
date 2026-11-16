//! Short ids that match more than one session or subagent, and the
//! `parent/agent-x` form `stats drill` prints.

mod common;

use common::{TempDir, claude_roots, session_env};
use peektrace_core::sessions::resolve::SessionError;
use peektrace_core::sessions::service::{Target, analyze_session, get_session};

const ONE: &str = "abcd1234-1111-4000-8000-000000000001";
const TWO: &str = "abcd1234-2222-4000-8000-000000000002";
const USER: &str = "{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n";
const SUB: &str = r#"{"type":"assistant","isSidechain":true,"requestId":"q1","message":{"model":"claude-opus-4-6","usage":{"input_tokens":10,"output_tokens":1}}}
"#;

/// Two sessions that share their first UUID part; the first has two
/// subagents that share a prefix.
fn tree() -> TempDir {
    let dir = TempDir::new("short-ids");
    dir.write(&format!("projects/-p/{ONE}.jsonl"), USER);
    dir.write(&format!("projects/-p/{TWO}.jsonl"), USER);
    for sub in ["agent-a1", "agent-a2"] {
        dir.write(&format!("projects/-p/{ONE}/subagents/{sub}.jsonl"), SUB);
    }
    dir
}

fn analyze(dir: &TempDir, id: &str, parent: Option<&str>) -> Result<String, SessionError> {
    let base = dir.str();
    let roots = claude_roots(&format!("{base}/home"), &format!("{base}/projects"));
    analyze_session(session_env(&roots), Target::Lookup(id), parent, true)
        .map(|a| a.parsed.session_id)
}

#[test]
fn a_shared_prefix_is_ambiguous_and_lists_both() {
    let dir = tree();
    let e = analyze(&dir, "abcd1234", None).expect_err("ambiguous");
    assert_eq!(
        e,
        SessionError::Ambiguous {
            id: "abcd1234".to_owned(),
            candidates: vec![ONE.to_owned(), TWO.to_owned()],
            count: 2,
        }
    );
    assert_eq!(
        e.to_string(),
        format!(
            "Session \"abcd1234\" is ambiguous: it matches 2 sessions ({ONE}, {TWO}). Give more of the id."
        )
    );
    assert_eq!(analyze(&dir, "abcd1234-1", None), Ok(ONE.to_owned()));
}

#[test]
fn a_shared_subagent_prefix_is_ambiguous() {
    let dir = tree();
    let e = analyze(&dir, "agent-a", Some(ONE)).expect_err("ambiguous");
    assert!(
        matches!(&e, SessionError::Ambiguous { candidates, count: 2, .. } if candidates == &["agent-a1", "agent-a2"]),
        "{e:?}"
    );
    assert_eq!(
        analyze(&dir, "agent-a2", Some(ONE)),
        Ok("agent-a2".to_owned())
    );
}

#[test]
fn the_drill_form_resolves_like_parent() {
    let dir = tree();
    assert_eq!(
        analyze(&dir, "abcd1234-1/agent-a1", None),
        Ok("agent-a1".to_owned())
    );
    let base = dir.str();
    let roots = claude_roots(&format!("{base}/home"), &format!("{base}/projects"));
    let p = get_session(
        session_env(&roots),
        Target::Lookup("abcd1234-1/agent-a2"),
        None,
        true,
    )
    .expect("session");
    assert_eq!(p.session_id, "agent-a2");
}

#[test]
fn the_drill_form_reports_an_ambiguous_parent() {
    let dir = tree();
    let e = analyze(&dir, "abcd1234/agent-a1", None).expect_err("ambiguous");
    assert!(
        matches!(e, SessionError::Ambiguous { count: 2, .. }),
        "{e:?}"
    );
    let e = analyze(&dir, "abcd1234-1/agent-zz", None).expect_err("missing");
    assert!(matches!(e, SessionError::NotFound { .. }), "{e:?}");
}

#[test]
fn two_copies_of_one_session_are_not_ambiguous() {
    let dir = tree();
    dir.write(&format!("projects/-q/{TWO}.jsonl"), USER);
    assert_eq!(analyze(&dir, "abcd1234-2", None), Ok(TWO.to_owned()));
}
