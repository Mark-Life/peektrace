//! Pi `context_edit` entries, ported from the TS test
//! `pi-context-edit.test.ts`. Same fixtures, same assertions. Cases already
//! held by `agents.rs` `pi_context_edits` are not repeated here.

mod common;

use common::{analyze_default, fixture, pretty, read_fixture};
use peektrace_core::js::units::est_tokens;
use peektrace_core::pi::parse_pi_session;
use peektrace_core::sessions::labels::context_edit_note;
use peektrace_core::sessions::model::{
    BudgetSlices, ContextEdit, ContextEditStep, EventKind, ParsedSession, TimelineEvent,
};
use peektrace_core::sessions::redact::redact_parsed;

const EDIT: &str = "sessions-pi/pi-context-edit.jsonl";
const STEPS: &str = "sessions-pi/pi-context-edit-steps.jsonl";

fn parse_text(text: &str) -> ParsedSession {
    parse_pi_session(text, &fixture(EDIT), "fallback")
}

fn parse() -> ParsedSession {
    parse_text(&read_fixture(EDIT))
}

fn steps() -> ParsedSession {
    parse_pi_session(&read_fixture(STEPS), &fixture(STEPS), "fallback")
}

/// Events built from the transcript line at `index`.
fn at(p: &ParsedSession, index: usize) -> Vec<&TimelineEvent> {
    p.events.iter().filter(|e| e.index == index).collect()
}

fn slices(p: ParsedSession, turn: usize) -> BudgetSlices {
    analyze_default(p).snapshots[turn].slices
}

/// `estTokens(JSON.stringify({ path: "big.txt" }, null, 2))`.
fn call() -> f64 {
    est_tokens(&pretty(&[("path", "big.txt")]))
}

#[test]
fn a_replaced_tool_result_shows_the_edit_and_keeps_the_original() {
    let p = parse();
    let ev = at(&p, 4)[0];
    assert_eq!(ev.body, "[big.txt elided]");
    assert_eq!(ev.tokens_est, est_tokens("[big.txt elided]"));
    assert_eq!(ev.tool_use_id.as_deref(), Some("t1"));
    assert_eq!(
        ev.context_edit,
        Some(ContextEdit::Replaced {
            at_index: 10.0,
            original_tokens_est: 1000.0,
            original: Some("X".repeat(4000)),
            steps: None,
        })
    );
}

#[test]
fn the_latest_edit_of_a_target_wins() {
    let p = parse();
    let ev = at(&p, 5)[0];
    assert!(ev.body.starts_with("reminder "), "{}", ev.body);
    assert_eq!(
        ev.context_edit,
        Some(ContextEdit::Removed {
            at_index: 12.0,
            steps: Some(vec![ContextEditStep {
                at_index: 11.0,
                tokens_est: est_tokens("first edit"),
                thinking_est: 0.0,
            }]),
        })
    );
}

#[test]
fn a_string_replacement_of_assistant_content_becomes_one_text_block() {
    let p = parse();
    let events = at(&p, 6);
    let kinds: Vec<_> = events.iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [EventKind::AssistantText]);
    assert_eq!(events[0].body, "Short summary.");
    match &events[0].context_edit {
        Some(ContextEdit::Replaced {
            original_tokens_est,
            ..
        }) => assert_eq!(
            *original_tokens_est,
            est_tokens("Summarizing the big file in detail.")
        ),
        other => panic!("not replaced: {other:?}"),
    }
}

#[test]
fn turns_before_the_edits_count_the_original_content() {
    let s = slices(parse(), 2);
    assert_eq!(s.tool_results, 1000.0);
    assert_eq!(
        s.other,
        est_tokens(&format!("reminder {}", "R".repeat(391)))
    );
}

#[test]
fn turns_after_the_edits_count_the_edited_content() {
    let s = slices(parse(), 3);
    assert_eq!(s.tool_results, est_tokens("[big.txt elided]"));
    assert_eq!(s.other, 0.0);
    assert_eq!(s.assistant_text, call() + est_tokens("Short summary."));
    assert_eq!(s.thinking, 50.0 - call());
}

#[test]
fn tags_and_notes_name_the_edit_line_and_keep_the_original() {
    let p = parse();
    let removed = at(&p, 8)[0];
    let replaced = at(&p, 4)[0];
    // TS `contextEditTag`: `removed` -> "removed", `replaced` -> "edited".
    let status = |e: &TimelineEvent| e.context_edit.as_ref().map(ContextEdit::status);
    assert_eq!(status(removed), Some("removed"));
    assert_eq!(status(replaced), Some("replaced"));
    let note = |e| context_edit_note(e).unwrap_or_default();
    assert!(note(removed).contains("line 10"));
    assert!(note(replaced).contains("~1000 tok"));
    assert!(note(replaced).ends_with(&"X".repeat(4000)));
}

#[test]
fn steps_turns_between_two_edits_count_the_first_replacement() {
    let s = slices(steps(), 2);
    assert_eq!(s.tool_results, est_tokens(&"Y".repeat(800)));
    assert_eq!(s.assistant_text, call() + est_tokens("short"));
}

#[test]
fn steps_turns_after_the_last_edit_count_the_final_state() {
    let s = slices(steps(), 3);
    assert_eq!(s.tool_results, 0.0);
    assert_eq!(s.assistant_text, call() + est_tokens("ok"));
}

#[test]
fn steps_turns_before_the_first_edit_count_the_original_as_measured() {
    let p = steps();
    assert_eq!(at(&p, 4)[0].tokens_measured, Some(1180.0));
    assert_eq!(slices(p, 1).tool_results, 1180.0);
}

#[test]
fn steps_the_note_names_every_edit_line() {
    let p = steps();
    assert_eq!(
        context_edit_note(at(&p, 4)[0]).as_deref(),
        Some(
            "Dropped from model context by the edit at line 10. Earlier edit at line 7. Later turns do not see it."
        )
    );
}

#[test]
fn the_replaced_original_is_redacted() {
    let secret = format!("sk-ant-api03-{}", "a1B2c3D4e5F6g7H8".repeat(6));
    let text = read_fixture(EDIT).replacen(&"X".repeat(40), &format!("api_key={secret} "), 1);
    let mut p = parse_text(&text);
    let raw = |p: &ParsedSession| match &at(p, 4)[0].context_edit {
        Some(ContextEdit::Replaced { original, .. }) => original.clone().unwrap_or_default(),
        _ => String::new(),
    };
    assert!(raw(&p).contains(&secret));
    redact_parsed(&mut p);
    match &at(&p, 4)[0].context_edit {
        Some(ContextEdit::Replaced { original, .. }) => {
            assert!(!original.as_deref().unwrap_or_default().contains(&secret));
        }
        other => panic!("not replaced: {other:?}"),
    }
}
