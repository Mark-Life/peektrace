//! The four corpus panels (context, fan-out, web repeats, cost) and the price
//! cards they read.
#![allow(clippy::float_cmp, reason = "token counts are whole numbers")]

mod common;

use common::{ctx_of, session_fact, tool_fact, window};
use peektrace_core::stats::accumulate::CorpusSummary;
use peektrace_core::stats::detectors::corpus::CorpusPanelSink;
use peektrace_core::stats::detectors::corpus_rows::CorpusPanels;
use peektrace_core::stats::redact::Redactor;
use peektrace_core::stats::schema::{CallFact, Fact, Kind, SessionFact};

fn session(sid: &str, edit: impl FnOnce(&mut SessionFact)) -> Fact {
    let mut f = session_fact();
    sid.clone_into(&mut f.base.sid);
    "p1".clone_into(&mut f.base.project);
    f.ended_at = Some("2026-08-01T01:00:00.000Z".into());
    f.models = vec!["claude-opus-5".into()];
    f.turns = 1.0;
    f.user_messages = 1.0;
    edit(&mut f);
    Fact::Session(f)
}

fn call(sid: &str, edit: impl FnOnce(&mut CallFact)) -> Fact {
    let mut f = tool_fact();
    sid.clone_into(&mut f.base.sid);
    "p1".clone_into(&mut f.base.project);
    "WebFetch".clone_into(&mut f.tool);
    edit(&mut f);
    Fact::Call(f)
}

fn run(facts: &[Fact]) -> (CorpusPanels, Vec<String>) {
    let mut sink = CorpusPanelSink::default();
    for f in facts {
        sink.add(f);
    }
    let (corpus, window, redact) = (CorpusSummary::default(), window(), Redactor::new(true));
    let mut ctx = ctx_of(&corpus, &window, &redact);
    let panels = sink.finish(&mut ctx);
    (panels, ctx.caveats)
}

fn caveat_with(caveats: &[String], needle: &str) -> String {
    caveats
        .iter()
        .find(|c| c.contains(needle))
        .cloned()
        .unwrap_or_default()
}

fn sids(p: &CorpusPanels) -> Vec<&str> {
    p.context.iter().map(|r| r.sid.as_str()).collect()
}

#[test]
fn context_ranks_by_peak_context() {
    let (p, _) = run(&[
        session("a", |f| {
            f.max_context_tokens = 100.0;
            f.cache_read_tokens = 9000.0;
        }),
        session("b", |f| {
            f.max_context_tokens = 900.0;
            f.cache_read_tokens = 10.0;
        }),
    ]);
    assert_eq!(sids(&p), ["b", "a"]);
}

#[test]
fn context_names_the_head_of_the_distribution() {
    let facts: Vec<Fact> = (0..100u32)
        .map(|i| {
            session(&format!("s{i:03}"), |f| {
                f.max_context_tokens = f64::from(i);
                f.cache_read_tokens = if i == 0 { 900_000.0 } else { 1010.0 };
            })
        })
        .collect();
    let (_, caveats) = run(&facts);
    let note = caveat_with(&caveats, "top 1%");
    assert!(
        note.contains("(1 of 100)") && note.contains("90.0%"),
        "{note}"
    );
}

#[test]
fn context_leaves_agent_transcripts_out() {
    let (p, caveats) = run(&[
        session("m", |f| {
            f.max_context_tokens = 10.0;
            f.cache_read_tokens = 100.0;
        }),
        session("w", |f| {
            f.base.kind = Kind::WorkflowAgent;
            f.max_context_tokens = 900.0;
            f.cache_read_tokens = 9000.0;
        }),
    ]);
    assert_eq!(sids(&p), ["m"]);
    let note = caveat_with(&caveats, "top 1%");
    assert!(
        note.contains("main sessions (1 of 1)") && note.contains("100.0%"),
        "{note}"
    );
}

#[test]
fn context_carries_sid_and_project() {
    let (p, _) = run(&[session("a", |f| {
        f.base.project = "proj".into();
        f.max_context_tokens = 5.0;
    })]);
    assert_eq!(p.context[0].key, "a");
    assert_eq!(p.context[0].project, "proj");
}

#[test]
fn two_transcripts_sharing_an_id_merge() {
    let (p, _) = run(&[
        session("a", |f| {
            f.max_context_tokens = 10.0;
            f.cache_read_tokens = 5.0;
        }),
        session("a", |f| {
            f.max_context_tokens = 30.0;
            f.cache_read_tokens = 7.0;
        }),
    ]);
    assert_eq!(p.context.len(), 1);
    assert_eq!(p.context[0].max_context_tokens, 30.0);
    assert_eq!(p.context[0].cache_read_tokens, 12.0);
}

fn fanout() -> Vec<Fact> {
    let agent = |sid: &str| {
        session(sid, |f| {
            f.base.kind = Kind::WorkflowAgent;
            f.base.run = Some("wf_1".into());
            f.first_turn_cache_creation = 30_000.0;
            f.cache_creation_tokens = 40_000.0;
        })
    };
    let read = |sid: &str, dur: f64| {
        call(sid, |f| {
            f.base.run = Some("wf_1".into());
            f.tool = "Read".into();
            f.dur_sec = Some(dur);
        })
    };
    vec![
        agent("a1"),
        agent("a2"),
        session("m1", |f| f.first_turn_cache_creation = 99_000.0),
        read("a1", 2.0),
        read("a2", 3.0),
    ]
}

#[test]
fn fanout_sums_the_prefix_per_run() {
    let (p, _) = run(&fanout());
    assert_eq!(p.fanout.len(), 1);
    let r = &p.fanout[0];
    assert_eq!(r.key, "wf_1");
    assert_eq!(
        (r.agents, r.prefix_tokens, r.tool_calls, r.total_sec),
        (2.0, 60_000.0, 2.0, 5.0)
    );
}

#[test]
fn fanout_states_the_kind_it_filtered_to() {
    let (_, caveats) = run(&fanout());
    let note = caveat_with(&caveats, "Fan-out");
    assert!(note.contains("75.0%"), "{note}");
    assert!(
        note.contains("2 workflow-agent transcripts across 1 runs"),
        "{note}"
    );
}

#[test]
fn a_main_session_is_not_a_fanout() {
    let (p, _) = run(&[session("m1", |f| f.first_turn_cache_creation = 99_000.0)]);
    assert_eq!(p.fanout, []);
}

const URL: &str = "https://example.com/docs";

fn web(sid: &str, run_id: Option<&str>, arg: &str, seq: f64) -> Fact {
    call(sid, |f| {
        f.base.run = run_id.map(str::to_owned);
        f.arg = Some(arg.to_owned());
        f.seq = seq;
    })
}

#[test]
fn a_repeat_inside_one_run_is_marked() {
    let (p, _) = run(&[
        web("a1", Some("wf_1"), URL, 0.0),
        web("a2", Some("wf_1"), URL, 1.0),
    ]);
    assert_eq!(p.web.len(), 1);
    let r = &p.web[0];
    assert_eq!((r.calls, r.sessions, r.same_run), (2.0, 2.0, true));
}

#[test]
fn the_same_url_from_two_runs_is_not_an_in_run_repeat() {
    let (p, _) = run(&[
        web("a1", Some("wf_1"), URL, 0.0),
        web("b1", Some("wf_2"), URL, 1.0),
    ]);
    assert!(!p.web[0].same_run);
}

#[test]
fn a_url_fetched_once_is_not_a_row() {
    let (p, _) = run(&[web("a1", None, URL, 0.0)]);
    assert_eq!(p.web, []);
}

#[test]
fn counts_in_run_repeats_not_calls() {
    let (_, caveats) = run(&[
        web("a1", Some("wf_1"), URL, 0.0),
        web("a1", Some("wf_1"), URL, 1.0),
        web("a1", Some("wf_1"), URL, 2.0),
        web("a1", Some("wf_1"), "https://example.com/other", 3.0),
    ]);
    assert!(caveat_with(&caveats, "Web:").contains("2 of 4 web calls"));
}

#[test]
fn a_non_web_tool_is_never_a_web_repeat() {
    let read = |seq: f64| {
        call("a1", |f| {
            f.tool = "Read".into();
            f.arg = Some("/tmp/x.ts".into());
            f.seq = seq;
        })
    };
    let (p, _) = run(&[read(0.0), read(1.0)]);
    assert_eq!(p.web, []);
}

fn cost_facts() -> Vec<Fact> {
    vec![
        session("m1", |f| {
            f.base.project = "alpha".into();
            f.output_tokens = 1e6;
        }),
        session("a1", |f| {
            f.base.kind = Kind::WorkflowAgent;
            f.base.project = "beta".into();
            f.models = vec!["claude-haiku-4-5".into()];
            f.output_tokens = 1e6;
        }),
    ]
}

#[test]
fn cost_rolls_up_by_kind_model_and_project() {
    let (p, _) = run(&cost_facts());
    let keys: Vec<&str> = p.cost.iter().map(|r| r.key.as_str()).collect();
    for k in [
        "kind:main",
        "kind:workflow-agent",
        "model:claude-opus-5",
        "project:alpha",
    ] {
        assert!(keys.contains(&k), "{k} in {keys:?}");
    }
}

#[test]
fn every_cost_label_marks_an_estimate() {
    let (p, _) = run(&cost_facts());
    assert!(p.cost.iter().all(|r| r.label.contains("est)")));
}

#[test]
fn the_report_echoes_the_rate_table() {
    let (_, caveats) = run(&cost_facts());
    let note = caveat_with(&caveats, "ESTIMATE");
    assert!(note.contains("opus-5 5/25/6.25/0.5"), "{note}");
    assert!(note.contains("haiku-4-5 1/5/1.25/0.1"), "{note}");
}

#[test]
fn an_unpriced_model_is_labelled_and_named() {
    let (p, caveats) = run(&[session("x", |f| {
        f.models = vec!["example-model-1".into()];
        f.output_tokens = 1000.0;
    })]);
    let r = p
        .cost
        .iter()
        .find(|r| r.key == "model:example-model-1")
        .expect("row");
    assert!(r.label.contains("(unpriced)"));
    assert!(caveat_with(&caveats, "no rate card").contains("example-model-1"));
}

#[test]
fn a_two_model_session_splits_evenly() {
    let (p, caveats) = run(&[session("x", |f| {
        f.models = vec!["claude-opus-5".into(), "claude-haiku-4-5".into()];
        f.output_tokens = 1000.0;
    })]);
    let outs: Vec<f64> = p
        .cost
        .iter()
        .filter(|r| r.key.starts_with("model:"))
        .map(|r| r.tokens.output)
        .collect();
    assert_eq!(outs, [500.0, 500.0]);
    assert!(caveat_with(&caveats, "splits each session").contains("1 of 1 sessions"));
}

#[test]
fn two_passes_produce_identical_output() {
    let facts = vec![
        session("b", |f| {
            f.max_context_tokens = 10.0;
            f.cache_read_tokens = 10.0;
        }),
        session("a", |f| {
            f.base.kind = Kind::WorkflowAgent;
            f.base.run = Some("wf_1".into());
            f.max_context_tokens = 10.0;
            f.cache_read_tokens = 10.0;
            f.cache_creation_tokens = 20.0;
            f.first_turn_cache_creation = 5.0;
        }),
        web("a", Some("wf_1"), "https://example.com", 0.0),
        web("b", None, "https://example.com", 1.0),
    ];
    assert_eq!(run(&facts), run(&facts));
}

#[test]
fn ties_break_on_the_key() {
    let (p, _) = run(&[
        session("z", |f| f.max_context_tokens = 1.0),
        session("a", |f| f.max_context_tokens = 1.0),
        session("m", |f| f.max_context_tokens = 1.0),
    ]);
    assert_eq!(sids(&p), ["a", "m", "z"]);
}
