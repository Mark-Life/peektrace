//! The ui crate copies core's label and cost helpers, since core runs them
//! on its own session model and the app on the wire types. These tests keep
//! the copies equal to core.

mod common;

use common::home;
use peektrace_core::js::time::Utc;
use peektrace_core::sessions::cost as core_cost;
use peektrace_core::sessions::labels as core_labels;
use peektrace_core::sessions::service::{SessionEnv, Target, analyze_session};
use peektrace_core::sessions::write::analyzed_json_with;
use peektrace_core::{AgentRoots, ListFilter, PathStyle, list_sessions};
use peektrace_protocol::view::AnalyzedSession;
use peektrace_ui::model::labels;

fn analyses() -> Vec<(
    peektrace_core::sessions::model::AnalyzedSession,
    AnalyzedSession,
)> {
    let home = home::build_home();
    let dir = home.to_string_lossy().into_owned();
    let env = peektrace_core::roots::RootsEnv::from_vars(dir, PathStyle::NATIVE, |_| None);
    let roots = AgentRoots::resolve(&env, &peektrace_core::roots::Settings::default());
    let listing = list_sessions(&roots, &ListFilter::default());
    let senv = SessionEnv {
        roots: &roots,
        local: &Utc,
    };
    listing
        .headers
        .iter()
        .filter_map(|h| analyze_session(senv, Target::Listed(h), None, true).ok())
        .map(|a| {
            let body = analyzed_json_with(&a, false);
            let view = serde_json::from_str(&peektrace_protocol::view::mend_surrogates(&body))
                .unwrap_or_else(|e| panic!("{e}"));
            (a, view)
        })
        .collect()
}

#[test]
fn labels_and_cost_match_core() {
    let all = analyses();
    assert!(all.len() >= 6, "{} analyses", all.len());
    for (core, view) in &all {
        assert_eq!(core.parsed.events.len(), view.events.len());
        for (c, v) in core.parsed.events.iter().zip(&view.events) {
            assert_eq!(
                labels::event_badge_label(v),
                core_labels::event_badge_label(c)
            );
            assert_eq!(labels::thinking_note(v), core_labels::thinking_note(c));
            assert_eq!(v.is_user_authored(), c.is_user_authored());
        }
        if let (Some(c), Some(v)) = (&core.cost, &view.cost) {
            let a = core_cost::cost_summary(c, "Claude Code");
            let b = labels::cost_summary(v, "Claude Code");
            assert_eq!((a.total, a.notes), (b.total, b.notes));
        }
    }
}
