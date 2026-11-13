//! Ports of `packages/viz/test/verdict-header-cost.test.tsx` and the summary
//! and timeline cases of `apps/inspector/test/transcript-render.test.tsx`,
//! rendered through Rune's `Runtime`.

use peektrace_protocol::view::{
    AnalyzedSession, BudgetSlices, CostSource, EventKind, SessionCost, SubagentRef, TimelineEvent,
    TurnSnapshot,
};
use rune::{ElementInfo, Env, Runtime};

use crate::model::fixtures::{analysis, kind, turn};
use crate::state::{Detail, Route, Selected, Ui};

fn render(a: AnalyzedSession) -> Runtime<Ui> {
    let mut s = Ui::new(None, 0.0, true);
    s.route = Route::Sessions;
    s.selected = Some(Selected {
        id: a.session_id.clone(),
        parent: None,
    });
    s.detail = Some(Detail::new(a.session_id.clone(), a, 1));
    let mut rt = Runtime::new(s, crate::views::view, (1440.0, 900.0), Env::new(true, 1.0));
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    rt
}

fn texts(els: &[ElementInfo]) -> String {
    els.iter()
        .filter_map(|e| e.text.clone().or_else(|| e.label.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn by_key<'a>(els: &'a [ElementInfo], key: &str) -> Option<&'a ElementInfo> {
    els.iter().find(|e| e.key.as_deref() == Some(key))
}

fn cost(source: CostSource) -> SessionCost {
    SessionCost {
        source,
        total_usd: None,
        subagents_usd: None,
        unpriced_models: None,
        unreadable_subagents: None,
    }
}

fn with_cost(c: SessionCost) -> AnalyzedSession {
    AnalyzedSession {
        cost: Some(c),
        ..analysis()
    }
}

// verdict-header-cost.test.tsx

#[test]
fn estimate_cost_renders_as_text_with_the_unpriced_models_named() {
    let rt = render(with_cost(SessionCost {
        total_usd: Some(1.23),
        unpriced_models: Some(vec!["mystery-model".into()]),
        ..cost(CostSource::Estimate)
    }));
    let els = rt.elements();
    let all = texts(&els);
    assert!(all.contains("EST. COST"), "{all}");
    assert!(all.contains("~$1.23"));
    assert!(all.contains("unpriced: mystery-model"));
    assert_eq!(
        by_key(&els, "session-cost").and_then(|e| e.help.as_deref()),
        Some("Estimate at list price")
    );
}

#[test]
fn agent_reported_opencode_cost_is_labelled_as_the_agents_figure() {
    let rt = render(AnalyzedSession {
        provider: "opencode".into(),
        ..with_cost(SessionCost {
            total_usd: Some(0.5),
            ..cost(CostSource::Agent)
        })
    });
    let els = rt.elements();
    let all = texts(&els);
    assert!(all.contains("~$0.50"), "{all}");
    assert!(all.contains("reported by OpenCode"));
    assert!(
        !els.iter()
            .any(|e| e.help.as_deref().is_some_and(|h| h.contains("list price")))
    );
}

#[test]
fn subagents_share_and_unreadable_subagents_show_as_notes() {
    let all = texts(
        &render(with_cost(SessionCost {
            total_usd: Some(2.0),
            subagents_usd: Some(0.75),
            unreadable_subagents: Some(1),
            ..cost(CostSource::Estimate)
        }))
        .elements(),
    );
    assert!(all.contains("subagents ~$0.75"), "{all}");
    assert!(all.contains("1 subagent(s) unreadable"));
}

#[test]
fn no_cost_means_no_cost_cell() {
    assert!(!texts(&render(analysis()).elements()).contains("EST. COST"));
}

#[test]
fn a_fully_unpriced_session_shows_a_dash_never_zero() {
    let all = texts(
        &render(with_cost(SessionCost {
            unpriced_models: Some(vec!["mystery-model".into()]),
            ..cost(CostSource::Estimate)
        }))
        .elements(),
    );
    assert!(all.contains("EST. COST"), "{all}");
    assert!(all.contains('—'));
    assert!(!all.contains("$0.00"));
}

// transcript-render.test.tsx: a summary worth jumping from, with a crossing,
// a compaction, a subagent and two plotted turns.

fn summary() -> AnalyzedSession {
    use EventKind as K;
    let evt = |index: usize, k: K, request: &str| TimelineEvent {
        index,
        request_id: (!request.is_empty()).then(|| request.to_owned()),
        ..kind(k)
    };
    let snap = |turn_index: usize| TurnSnapshot {
        turn_index,
        model: "claude-opus-4".into(),
        ctx: 40_000.0,
        output_tokens: 0.0,
        cache_read_tokens: 0.0,
        slices: BudgetSlices::default(),
        ts: None,
    };
    AnalyzedSession {
        compaction_indexes: vec![2],
        compaction_turns: vec![1],
        dumb_zone_cross_turn: 1,
        dumb_zone_fraction: 0.5,
        dumb_zone_turns: 1,
        events: vec![
            evt(0, K::UserPrompt, "r1"),
            evt(1, K::AssistantText, "r1"),
            evt(2, K::Compaction, ""),
            evt(3, K::UserPrompt, "r2"),
            evt(4, K::AssistantText, "r2"),
        ],
        snapshots: vec![snap(0), snap(1)],
        subagents: vec![SubagentRef {
            id: "agent-1".into(),
            agent_type: None,
            description: None,
            tool_use_id: None,
            turns: 1,
            peak_context_tokens: 0.0,
        }],
        turn_count: 2,
        turns: vec![turn("r1"), turn("r2")],
        ..analysis()
    }
}

#[test]
fn summary_chips_are_jump_buttons_named_by_their_text() {
    let rt = render(summary());
    let els = rt.elements();
    let chip = |k| by_key(&els, k).unwrap_or_else(|| panic!("no {k}"));
    for k in ["chip-cross", "chip-compactions"] {
        assert_eq!(chip(k).role, "button", "{k}");
        assert!(chip(k).focusable, "{k}");
    }
    assert_eq!(
        chip("chip-cross").label.as_deref(),
        Some("entered dumb zone @ turn 2, jump to that turn in history")
    );
    assert_eq!(
        chip("chip-compactions").label.as_deref(),
        Some("1 compaction(s), jump to the first in history")
    );
    // No spawning call in this transcript and no subagents list in this
    // slice, so the subagent chip has nowhere to jump.
    assert_ne!(chip("chip-subagents").role, "button");
}

#[test]
fn the_timeline_takes_clicks_and_focus_on_its_two_markers() {
    let rt = render(summary());
    let els = rt.elements();
    assert!(by_key(&els, "growth-hit").is_some());
    let markers: Vec<&ElementInfo> = els
        .iter()
        .filter(|e| {
            e.focusable
                && e.key
                    .as_deref()
                    .is_some_and(|k| k.starts_with("growth-"))
        })
        .collect();
    assert_eq!(markers.len(), 2, "{markers:?}");
    let labels: Vec<&str> = markers.iter().filter_map(|e| e.label.as_deref()).collect();
    assert!(
        labels
            .iter()
            .any(|l| l.starts_with("Entered dumb zone at turn 2: jump"))
    );
    assert!(labels.iter().any(|l| l.starts_with("Peak ")));
}
