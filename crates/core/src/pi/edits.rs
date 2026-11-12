//! Pi `context_edit` entries: append-only edits to an earlier entry's share
//! of model context. `replacement: null` drops the target; otherwise its
//! content is swapped. The latest edit per target wins; earlier ones still
//! shape the context between their lines.

use std::collections::HashMap;

use super::parse::State;
use crate::js::value::{EMPTY_OBJECT, JsObject, JsValue};
use crate::sessions::model::{ContextEdit, ContextEditStep, EventKind, TimelineEvent, Turn};

/// One edit of a target entry.
#[derive(Debug, Clone, PartialEq)]
pub struct PiEdit {
    /// Row index of the `context_edit` entry.
    pub at_index: usize,
    /// Replacement content; `None` when the target is dropped.
    pub content: Option<JsValue>,
}

fn parse_edit(row: &JsValue, at_index: usize) -> Option<PiEdit> {
    match row.get("replacement")? {
        JsValue::Null => Some(PiEdit {
            at_index,
            content: None,
        }),
        JsValue::Obj(r) => match r.get("content") {
            Some(c @ (JsValue::Str(_) | JsValue::Arr(_))) => Some(PiEdit {
                at_index,
                content: Some(c.clone()),
            }),
            _ => None,
        },
        _ => None,
    }
}

/// Edits per target entry id, oldest first. Rows `keep` rejects (off the
/// active branch) are never applied.
pub fn collect_edits(
    rows: &[JsValue],
    keep: impl Fn(usize) -> bool,
) -> HashMap<String, Vec<PiEdit>> {
    let mut edits: HashMap<String, Vec<PiEdit>> = HashMap::new();
    for (at, row) in rows.iter().enumerate() {
        if row.get("type").and_then(JsValue::as_str) != Some("context_edit") || !keep(at) {
            continue;
        }
        let Some(target) = row.get("targetId").and_then(JsValue::as_str) else {
            continue;
        };
        if let Some(edit) = parse_edit(row, at) {
            edits.entry(target.to_owned()).or_default().push(edit);
        }
    }
    edits
}

fn role_of(row: &JsValue) -> String {
    super::context::string_or(row.get("message").and_then(|m| m.get("role")), "undefined")
}

/// True when Pi would apply a context edit to this row.
pub fn is_editable(row: &JsValue) -> bool {
    match row.get("type").and_then(JsValue::as_str) {
        Some("custom_message") => true,
        Some("message") => matches!(role_of(row).as_str(), "user" | "assistant" | "toolResult"),
        _ => false,
    }
}

/// A copy of `row` with its content swapped for `content`. A string becomes
/// one text block for assistant and tool-result messages.
fn replaced_line(row: &JsValue, content: &JsValue) -> JsValue {
    let obj = row.as_obj().cloned().unwrap_or_default();
    if row.get("type").and_then(JsValue::as_str) == Some("custom_message") {
        return JsValue::Obj(obj.with("content", content.clone()));
    }
    let msg = match row.get("message") {
        Some(JsValue::Obj(m)) => m.clone(),
        _ => EMPTY_OBJECT.as_obj().cloned().unwrap_or_default(),
    };
    let normalized = match content {
        JsValue::Str(_) if matches!(role_of(row).as_str(), "assistant" | "toolResult") => {
            JsValue::Arr(vec![JsValue::Obj(JsObject::from_entries([
                ("type".to_owned(), JsValue::Str("text".to_owned())),
                ("text".to_owned(), content.clone()),
            ]))])
        }
        c => c.clone(),
    };
    JsValue::Obj(obj.with("message", JsValue::Obj(msg.with("content", normalized))))
}

/// Drop turn links to events at or after `start` (they are being rebuilt).
fn unlink_from(turns: &mut [Turn], start: usize) {
    #[expect(clippy::cast_possible_wrap, reason = "positions fit i64")]
    let start = start as i64;
    for t in turns {
        t.event_indexes.retain(|&i| i < start);
    }
}

/// Visible and thinking size of one entry's events.
fn entry_size(events: &[TimelineEvent]) -> (f64, f64) {
    events.iter().fold((0.0, 0.0), |(visible, thinking), e| {
        if e.kind == EventKind::AssistantThinking {
            (visible, thinking + e.tokens_est)
        } else {
            (visible + e.tokens_est, thinking)
        }
    })
}

#[expect(clippy::cast_precision_loss, reason = "row indexes fit f64")]
fn at(i: usize) -> f64 {
    i as f64
}

/// Steps for the event at position `i` of an entry: sizes live on the first.
fn steps_for(steps: &[ContextEditStep], i: usize) -> Option<Vec<ContextEditStep>> {
    if steps.is_empty() {
        return None;
    }
    Some(if i == 0 {
        steps.to_vec()
    } else {
        steps
            .iter()
            .map(|s| ContextEditStep {
                at_index: s.at_index,
                tokens_est: 0.0,
                thinking_est: 0.0,
            })
            .collect()
    })
}

fn mark_removed(
    events: Vec<TimelineEvent>,
    at_index: usize,
    steps: &[ContextEditStep],
) -> Vec<TimelineEvent> {
    events
        .into_iter()
        .enumerate()
        .map(|(i, mut e)| {
            e.context_edit = Some(ContextEdit::Removed {
                at_index: at(at_index),
                steps: steps_for(steps, i),
            });
            e
        })
        .collect()
}

/// The first replacement event carries the original text and its visible
/// size (thinking is costed from output tokens instead).
fn mark_replaced(
    original: &[TimelineEvent],
    replacement: Vec<TimelineEvent>,
    at_index: usize,
    steps: &[ContextEditStep],
) -> Vec<TimelineEvent> {
    let text = original
        .iter()
        .map(|e| e.body.as_str())
        .filter(|b| !b.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let (visible, _) = entry_size(original);
    replacement
        .into_iter()
        .enumerate()
        .map(|(i, mut e)| {
            e.context_edit = Some(ContextEdit::Replaced {
                at_index: at(at_index),
                original_tokens_est: if i == 0 { visible } else { 0.0 },
                original: (i == 0).then(|| text.clone()),
                steps: steps_for(steps, i),
            });
            e
        })
        .collect()
}

/// Runs one row into the state.
pub type Run<'a> = dyn FnMut(&JsValue, &mut State) + 'a;

/// Events of `row` with `content` swapped in; the state is left as before.
fn probe(
    state: &mut State,
    row: &JsValue,
    content: &JsValue,
    run: &mut Run<'_>,
) -> Vec<TimelineEvent> {
    let start = state.events.len();
    run(&replaced_line(row, content), state);
    let events = state.events.split_off(start);
    unlink_from(&mut state.turns, start);
    events
}

/// Fold one row via `run`, then apply its context edits to the new events.
/// Reruns repeat `run`'s other effects, which are all idempotent.
pub fn fold_with_edit(
    state: &mut State,
    row: &JsValue,
    edits: Option<&[PiEdit]>,
    run: &mut Run<'_>,
) {
    let start = state.events.len();
    run(row, state);
    let Some((edit, earlier)) = edits.and_then(<[PiEdit]>::split_last) else {
        return;
    };
    let original = state.events.split_off(start);
    unlink_from(&mut state.turns, start);
    let steps: Vec<ContextEditStep> = earlier
        .iter()
        .map(|step| {
            let (tokens_est, thinking_est) = step
                .content
                .as_ref()
                .map_or((0.0, 0.0), |c| entry_size(&probe(state, row, c, run)));
            ContextEditStep {
                at_index: at(step.at_index),
                tokens_est,
                thinking_est,
            }
        })
        .collect();
    if let Some(content) = &edit.content {
        run(&replaced_line(row, content), state);
        let replacement = state.events.split_off(start);
        if !replacement.is_empty() {
            let marked = mark_replaced(&original, replacement, edit.at_index, &steps);
            state.events.extend(marked);
            return;
        }
        unlink_from(&mut state.turns, start);
    }
    run(row, state);
    state.events.truncate(start);
    state
        .events
        .extend(mark_removed(original, edit.at_index, &steps));
}
