//! The ui crate copies core's label and cost helpers, since core runs them
//! on its own session model and the app on the wire types. These tests keep
//! the copies equal to core.

#![allow(
    clippy::assert_is_empty,
    reason = "the keys print on failure where it matters"
)]

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

#[test]
fn stats_retry_window_matches_core() {
    assert!(
        (peektrace_ui::model::stats::RETRY_WINDOW
            - peektrace_core::stats::detectors::pairing::RETRY_WINDOW)
            .abs()
            < f64::EPSILON
    );
}

#[test]
fn the_report_view_keeps_every_row_core_writes() {
    use peektrace_core::stats::cache::{StatsCache, cache_root};
    use peektrace_core::stats::service::{ReportRequest, StatsEnv, drill, report};
    use peektrace_protocol::stats::{DrillView, StatsReportView};
    let home = home::stats_home();
    let dir = home.to_string_lossy().into_owned();
    let env = peektrace_core::roots::RootsEnv::from_vars(dir, PathStyle::NATIVE, |_| None);
    let roots = AgentRoots::resolve(&env, &peektrace_core::roots::Settings::default());
    #[expect(clippy::cast_precision_loss, reason = "epoch ms")]
    let now = peektrace_js::time::parse_date(home::NOW, &Utc).unwrap_or(0) as f64;
    let senv = StatsEnv {
        roots: &roots,
        cache: StatsCache::new(cache_root(&env)),
        local: &Utc,
        now_ms: now,
        progress: None,
    };
    let req = ReportRequest {
        agents: None,
        days: Some(7.0),
        force: false,
        limit: None,
    };
    let r = report(&senv, &req).unwrap_or_else(|e| panic!("{e:?}"));
    let v: StatsReportView =
        serde_json::from_str(&r.to_json(false)).unwrap_or_else(|e| panic!("{e}"));
    assert!(!r.causes.is_empty());
    assert_eq!(v.causes.len(), r.causes.len());
    assert_eq!(v.fails.len(), r.fails.len());
    assert_eq!(v.time.len(), r.time.len());
    assert_eq!(v.clusters.len(), r.clusters.len());
    assert_eq!(v.caveats, r.caveats);
    for (a, b) in v.causes.iter().zip(&r.causes) {
        assert_eq!(a.key, b.key);
        assert_eq!(a.samples.len(), b.samples.len());
        assert_eq!(a.rule.is_some(), b.rule.is_some());
    }
    let text = peektrace_ui::model::stats::derive::text_of(&r, None);
    assert_eq!(
        text.findings.len(),
        peektrace_core::stats::findings::findings_of(&r).len()
    );
    assert_eq!(text.prompts.cause.len(), r.causes.len());
    let key = &r.causes[0].key;
    let page = peektrace_core::stats::service::DrillPage {
        offset: None,
        page_size: None,
    };
    let d = drill(
        &senv,
        &req,
        peektrace_core::stats::accumulate::DrillTable::Cause,
        key,
        page,
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let dv: DrillView = serde_json::from_str(&d.to_json(false)).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(dv.hits.len(), d.hits.len());
    assert_eq!(dv.key, *key);
}
