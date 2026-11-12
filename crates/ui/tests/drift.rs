//! The ui crate copies a few core helpers because core does not build for
//! wasm32. These tests keep the copies equal to core.

mod common;

use common::home;
use peektrace_core::js::num as core_num;
use peektrace_core::js::time::{Utc, parse_date};
use peektrace_core::sessions::cost as core_cost;
use peektrace_core::sessions::labels as core_labels;
use peektrace_core::sessions::service::{SessionEnv, Target, analyze_session};
use peektrace_core::sessions::write::analyzed_json_with;
use peektrace_core::{AgentRoots, ListFilter, PathStyle, list_sessions};
use peektrace_protocol::view::AnalyzedSession;
use peektrace_ui::model::{jsnum, jstime, labels};

/// A deterministic sweep: ties, halves, near-powers, negatives, tiny and huge.
fn sweep() -> Vec<f64> {
    let mut out = vec![
        0.0,
        -0.0,
        0.5,
        1.25,
        1.35,
        2.5,
        999_999.5,
        1e6 - 1.0,
        1e6 + 1.0,
        1e21,
        1e-7,
        f64::NAN,
        f64::INFINITY,
    ];
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    for i in 0..100_000_u32 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        #[expect(clippy::cast_precision_loss, reason = "a test sweep")]
        let base = (x % 10_000_000) as f64 / 1000.0;
        out.push(if i % 3 == 0 { -base } else { base });
        out.push(f64::from(i) + 0.05);
    }
    out
}

#[test]
fn numbers_match_core() {
    for x in sweep() {
        assert_eq!(jsnum::to_js_string(x), core_num::to_js_string(x), "{x:?}");
        for f in [0, 1, 2] {
            assert_eq!(jsnum::to_fixed(x, f), core_num::to_fixed(x, f), "{x:?} {f}");
        }
        assert_eq!(
            jsnum::js_round(x).to_bits(),
            core_num::js_round(x).to_bits(),
            "{x:?}"
        );
    }
}

#[test]
fn dates_match_core() {
    for s in [
        "2026-06-01T10:03:00Z",
        "2026-06-01T10:03:00.123+02:00",
        "2026-02-30",
        "2026-13-01",
        "nope",
        "2026-06-01T24:00",
        "+002026-06-01T00:00:00Z",
    ] {
        assert_eq!(jstime::parse_date(s), parse_date(s, &Utc), "{s}");
    }
}

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
