//! The JSON readers invert the writers: `write(read(write(x))) == write(x)`
//! for every fixture session (and, with `--ignored`, the real corpus).

mod common;

use peektrace_core::js::time::Utc;
use peektrace_core::js::value::JsValue;
use peektrace_core::roots::{RootsEnv, Settings};
use peektrace_core::sessions::read::parsed_from_json;
use peektrace_core::sessions::service::{SessionEnv, Target, get_session};
use peektrace_core::sessions::write::parsed_json;
use peektrace_core::{AgentRoots, ListFilter, PathStyle, list_sessions};

/// Sessions checked and the ids that did not round-trip.
fn round_trip_all(roots: &AgentRoots) -> (usize, Vec<String>) {
    let env = SessionEnv { roots, local: &Utc };
    let listing = list_sessions(roots, &ListFilter::default());
    let mut failed = Vec::new();
    let mut checked = 0;
    for h in &listing.headers {
        for redact in [true, false] {
            let Ok(p) = get_session(env, Target::Listed(h), None, redact) else {
                continue;
            };
            checked += 1;
            let text = parsed_json(&p, false);
            let back = JsValue::parse(&text).as_ref().and_then(parsed_from_json);
            if back.as_ref().map(|b| parsed_json(b, false)).as_deref() != Some(text.as_str()) {
                failed.push(h.id.as_str().to_owned());
            }
        }
    }
    (checked, failed)
}

#[test]
fn fixtures_round_trip() {
    let env = RootsEnv {
        home: "/nonexistent".to_owned(),
        claude_projects: Some(common::fixture("sessions/projects")),
        codex_sessions: Some(common::fixture("sessions-codex")),
        pi_sessions: Some(common::fixture("sessions-pi")),
        opencode_data: Some(common::fixture("sessions-opencode")),
        ..RootsEnv::default()
    };
    let roots = AgentRoots::resolve(&env, &Settings::default());
    let (checked, failed) = round_trip_all(&roots);
    assert!(checked > 4, "only {checked} sessions");
    assert_eq!(failed, Vec::<String>::new());
    let all = RootsEnv {
        claude_projects: Some(common::fixture("")),
        codex_sessions: Some(common::fixture("")),
        pi_sessions: Some(common::fixture("")),
        ..env
    };
    let (_, failed) = round_trip_all(&AgentRoots::resolve(&all, &Settings::default()));
    assert_eq!(failed, Vec::<String>::new());
}

#[test]
#[ignore = "reads the real corpus under HOME"]
fn real_corpus_round_trip() {
    let home = std::env::home_dir()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_default();
    let env = RootsEnv::from_vars(home, PathStyle::NATIVE, |k| std::env::var(k).ok());
    let roots = AgentRoots::resolve(&env, &Settings::default());
    let (checked, failed) = round_trip_all(&roots);
    eprintln!("checked {checked} parses");
    assert_eq!(failed, Vec::<String>::new());
}
