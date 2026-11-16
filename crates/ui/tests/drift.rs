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
            assert_eq!(
                (a.total, a.notes, a.warning, a.estimate_note),
                (b.total, b.notes, b.warning, b.estimate_note)
            );
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

/// The server answers `/api/capabilities`, `/api/server` and
/// `/api/settings` through `peektrace_core::wire` (its own tests hold it to
/// that); the in-process source must answer through the same functions.
#[test]
fn in_process_replies_are_the_server_wire_bodies() {
    use peektrace_core::settings_doc;
    use peektrace_core::wire;
    use peektrace_protocol::{SettingsResult, SettingsUpdate};
    use peektrace_ui::source::in_process::InProcess;
    use peektrace_ui::source::{DataSource as _, LoadError, decode};

    let home = home::temp_home("drift-wire");
    let dir = home.to_string_lossy().into_owned();
    let env = peektrace_core::roots::RootsEnv::from_vars(dir.clone(), PathStyle::NATIVE, |_| None);
    let roots = AgentRoots::resolve(&env, &peektrace_core::roots::Settings::default());
    let settings_path = env.settings_path();

    for read_only in [false, true] {
        let src = InProcess::for_home(dir.clone(), |_| None).with_read_only(read_only);
        assert_eq!(
            src.capabilities_now().and_then(Result::ok),
            Some(wire::capability_rows())
        );
        assert_eq!(
            src.server_info_now().and_then(Result::ok),
            Some(wire::server_info(&roots, read_only, &dir))
        );
        let want: Option<SettingsResult> = decode(&wire::settings_body(
            &settings_doc::read_stored(&settings_path),
        ))
        .ok();
        assert_eq!(src.settings_now().and_then(Result::ok), want);
    }

    let update = |settings: serde_json::Value, expected_mtime| SettingsUpdate {
        settings: serde_json::from_value(settings).unwrap_or_default(),
        expected_mtime,
    };
    let locked = InProcess::for_home(dir.clone(), |_| None).with_read_only(true);
    assert_eq!(
        locked
            .save_settings_now(update(serde_json::json!({}), None))
            .and_then(Result::err),
        wire::check_settings_writable(true)
            .err()
            .map(LoadError::Api)
    );
    let src = InProcess::for_home(dir, |_| None);
    let bad = serde_json::json!({"roots": {"claude": [{"label": "x"}]}});
    let bad_value =
        peektrace_core::js::value::JsValue::parse(&bad.to_string()).expect("bad is JSON");
    assert_eq!(
        src.save_settings_now(update(bad, None))
            .and_then(Result::err),
        wire::validated_settings(&bad_value)
            .err()
            .map(LoadError::Api)
    );
    let saved = src
        .save_settings_now(update(serde_json::json!({}), Some(0.0)))
        .and_then(Result::ok);
    let want: Option<SettingsResult> = decode(&wire::settings_body(&settings_doc::read_stored(
        &settings_path,
    )))
    .ok();
    assert_eq!(saved, want);
    assert_eq!(
        src.save_settings_now(update(serde_json::json!({}), Some(1.0)))
            .and_then(Result::err),
        wire::store_settings(
            &settings_path,
            settings_doc::SettingsDoc::default(),
            Some(1.0)
        )
        .err()
        .map(LoadError::Api)
    );
}
