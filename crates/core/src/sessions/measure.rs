//! Measured token sizes for context added between two model calls. Each
//! call's usage gives the real prompt size, so what the harness added
//! between call N and N+1 is `ctx(N+1) - ctx(N) - output(N)`. That delta
//! goes to the events added in between, split by estimate. Steps the delta
//! cannot describe (compaction, prompt change, context edits, subagent
//! lines, negative deltas) keep their chars/4 estimates.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use super::images::IMAGE_MARKER;
use super::model::{EventKind, ParsedSession, TimelineEvent, Turn};

/// Split weight of one image, near a full-size screenshot on Claude.
const IMAGE_WEIGHT: f64 = 1500.0;

/// Events that reset or rewrite context, so no step can span them.
fn is_boundary(e: &TimelineEvent) -> bool {
    matches!(
        e.kind,
        EventKind::Compaction | EventKind::SystemPrompt | EventKind::Summary
    )
}

#[expect(clippy::cast_precision_loss, reason = "JS numbers are f64")]
fn split_weight(e: &TimelineEvent) -> f64 {
    e.tokens_est + IMAGE_WEIGHT * e.body.matches(IMAGE_MARKER).count() as f64
}

/// Line indexes at which some context edit takes effect.
fn edit_lines(events: &[TimelineEvent]) -> Vec<f64> {
    events
        .iter()
        .filter_map(|e| e.context_edit.as_ref())
        .flat_map(|c| std::iter::once(c.at_index()).chain(c.steps().iter().map(|s| s.at_index)))
        .collect()
}

/// `Array.prototype.slice(0, n)` length for a list of `len` items.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "slice bounds are clamped to the list"
)]
fn slice_len(n: f64, len: usize) -> usize {
    let k = n.trunc();
    let len_f = len as f64;
    if k.is_nan() {
        0
    } else if k < 0.0 {
        (len_f + k).max(0.0) as usize
    } else {
        k.min(len_f) as usize
    }
}

/// Split `delta` over `added` by weight, largest remainder first, so shares
/// stay >= 0 and sum to `delta`. `None` when no weight.
pub fn split_delta(events: &[TimelineEvent], added: &[usize], delta: f64) -> Option<Vec<f64>> {
    let weights: Vec<f64> = added.iter().map(|&i| split_weight(&events[i])).collect();
    let total = weights.iter().fold(0.0, |a, b| a + b);
    if total <= 0.0 {
        return None;
    }
    let exact: Vec<f64> = weights.iter().map(|w| (delta * w) / total).collect();
    let mut shares: Vec<f64> = exact.iter().map(|x| x.floor()).collect();
    let left = delta - shares.iter().fold(0.0, |a, b| a + b);
    let mut order: Vec<(usize, f64)> = exact
        .iter()
        .enumerate()
        .map(|(i, x)| (i, x - x.floor()))
        .collect();
    order.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    for &(i, _) in order.iter().take(slice_len(left, order.len())) {
        shares[i] += 1.0;
    }
    Some(shares)
}

/// One gap between two consecutive model calls.
struct Step {
    added: Vec<usize>,
    clean: bool,
    delta: f64,
}

#[expect(clippy::cast_precision_loss, reason = "line indexes fit f64")]
fn line_of(e: &TimelineEvent) -> f64 {
    e.index as f64
}

fn collect_steps(p: &ParsedSession) -> Vec<Step> {
    let events = &p.events;
    let turn_by_req: HashMap<&str, &Turn> =
        p.turns.iter().map(|t| (t.request_id.as_str(), t)).collect();
    let mut seen = HashSet::new();
    let starts: Vec<(usize, &Turn)> = events
        .iter()
        .enumerate()
        .filter_map(|(pos, e)| {
            let req = e.request_id.as_deref().filter(|r| !r.is_empty())?;
            let turn = *turn_by_req.get(req)?;
            seen.insert(turn.request_id.as_str()).then_some((pos, turn))
        })
        .collect();
    let edits = edit_lines(events);
    #[expect(clippy::cast_precision_loss, reason = "line indexes fit f64")]
    let compactions: Vec<f64> = p.compaction_indexes.iter().map(|&i| i as f64).collect();
    starts
        .windows(2)
        .map(|pair| {
            let ((prev_pos, prev), (next_pos, next)) = (pair[0], pair[1]);
            let prev_line = line_of(&events[prev_pos]);
            let next_line = line_of(&events[next_pos]);
            let between = &events[prev_pos + 1..next_pos];
            let added = between
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    let has_req = e.request_id.as_deref().is_some_and(|r| !r.is_empty());
                    !(has_req || e.kind.is_output()) && split_weight(e) > 0.0
                })
                .map(|(i, _)| prev_pos + 1 + i)
                .collect::<Vec<_>>();
            let side = events[prev_pos].sidechain();
            let foreign = between.iter().any(|e| {
                let req = e.request_id.as_deref().filter(|r| !r.is_empty());
                e.sidechain() != side
                    || is_boundary(e)
                    || matches!(
                        e.context_edit,
                        Some(super::model::ContextEdit::Replaced { .. })
                    )
                    || req.map_or(e.kind.is_output(), |r| !turn_by_req.contains_key(r))
            });
            let crosses = |line: &f64| *line >= prev_line && *line < next_line;
            let delta = next.context_tokens - prev.context_tokens - prev.output_tokens;
            Step {
                clean: !foreign
                    && events[next_pos].sidechain() == side
                    && delta >= 0.0
                    && !added.is_empty()
                    && !edits.iter().any(crosses)
                    && !compactions.iter().any(crosses),
                added,
                delta,
            }
        })
        .collect()
}

/// Set `tokensMeasured` on every event whose size the usage pins down.
pub fn measure_tokens(p: &mut ParsedSession) {
    let mut measured = Vec::new();
    for step in collect_steps(p).into_iter().filter(|s| s.clean) {
        if let Some(shares) = split_delta(&p.events, &step.added, step.delta) {
            measured.extend(step.added.iter().copied().zip(shares));
        }
    }
    for (pos, share) in measured {
        p.events[pos].tokens_measured = Some(share);
    }
}
