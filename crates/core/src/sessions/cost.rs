//! Session cost from priced turns: a list-price estimate unless the agent
//! recorded its own figure.

use super::model::{
    CostParts, ParsedSession, Provider, SessionCost, SubagentRef, Turn, TurnField, TurnsCost,
};
use crate::js::text;
use crate::model::HeaderCost;
use crate::pricing::{CostTokens, card_for, turn_cost};

/// A turn's cost at list price, `None` when its model is unpriced.
fn price(t: &Turn) -> Option<f64> {
    let tokens = CostTokens {
        input: t.input_tokens,
        output: t.output_tokens,
        cache_read: t.cache_read_tokens,
        cache_creation: t.cache_creation_tokens,
        cache_creation_1h: t.cache_creation_1h_tokens,
    };
    turn_cost(card_for(&t.model).as_ref(), &tokens)
}

/// Set `costUsd` on each turn that has none yet.
pub fn price_turns(turns: &mut [Turn]) {
    for t in turns.iter_mut().filter(|t| t.cost_usd.is_none()) {
        if let Some(usd) = price(t) {
            t.cost_usd = Some(usd);
            t.appended.push(TurnField::CostUsd);
        }
    }
}

/// Distinct names in UTF-16 order, or `None` when empty.
fn sorted(mut names: Vec<String>) -> Option<Vec<String>> {
    names.sort_by(|a, b| text::cmp_utf16(a, b));
    names.dedup();
    (!names.is_empty()).then_some(names)
}

/// Sum of priced turns; an unpriced turn names its model.
fn sum_priced(turns: &[Turn]) -> TurnsCost {
    let mut usd = None::<f64>;
    let mut unpriced = Vec::new();
    for t in turns {
        if let Some(c) = t.cost_usd {
            usd = Some(usd.unwrap_or(0.0) + c);
        } else {
            unpriced.push(t.model.clone());
        }
    }
    TurnsCost {
        cost_usd: usd,
        unpriced_models: sorted(unpriced),
    }
}

/// Cost fields of a turn list: a subagent or inline sidechain turns.
pub fn turns_cost(turns: &[Turn]) -> TurnsCost {
    let mut priced = turns.to_vec();
    price_turns(&mut priced);
    sum_priced(&priced)
}

/// Record and estimate differ past this share: `estimate_gap` is set.
pub const ESTIMATE_GAP_LIMIT: f64 = 0.05;

/// Warn when the estimate is above the record by more than this: a price
/// card may be too high, or calls are counted twice.
pub const ESTIMATE_HIGH_WARN: f64 = 0.05;

/// Warn when the estimate is below the record by more than this: a price
/// may be missing or wrong. Smaller gaps are normal, because transcripts
/// miss some billed calls.
pub const ESTIMATE_LOW_WARN: f64 = 0.35;

/// `(estimate - recorded) / recorded`, `None` without a positive record.
fn gap(recorded: f64, estimate: f64) -> Option<f64> {
    (recorded > 0.0).then(|| (estimate - recorded) / recorded)
}

/// The gap when it passes `ESTIMATE_GAP_LIMIT`.
fn estimate_gap(recorded: f64, estimate: f64) -> Option<f64> {
    gap(recorded, estimate).filter(|g| g.abs() > ESTIMATE_GAP_LIMIT)
}

/// True when a full record and our estimate disagree in a way that points
/// at a price problem: an unpriced model, an estimate too high, or one far
/// too low.
pub fn estimate_warns(recorded: f64, estimate: f64, unpriced: bool) -> bool {
    gap(recorded, estimate)
        .is_some_and(|g| unpriced || g > ESTIMATE_HIGH_WARN || g < -ESTIMATE_LOW_WARN)
}

/// A session's cost. The agent's own record is the total when it covers the
/// whole session; a record that already counts subagents gets nothing added.
/// Otherwise the sum of priced turns, inline sidechain turns and subagents.
/// `None` when there is nothing to price or report.
pub fn cost_of(c: &CostParts) -> Option<SessionCost> {
    let own = &c.own;
    let subagents_usd = c
        .parts
        .iter()
        .fold(0.0, |n, p| n + p.cost_usd.unwrap_or(0.0));
    let parts_priced = c.parts.iter().any(|p| p.cost_usd.is_some());
    let native = c.native;
    let headline = native.filter(|n| !n.partial);
    // Our estimate over what the record covers.
    let estimate = native.map(|n| {
        own.cost_usd.unwrap_or(0.0)
            + if n.includes_subagents {
                subagents_usd
            } else {
                0.0
            }
    });
    let total = match headline {
        Some(n) if n.includes_subagents => Some(n.usd),
        Some(n) => Some(n.usd + subagents_usd),
        None => (own.cost_usd.is_some() || parts_priced)
            .then(|| own.cost_usd.unwrap_or(0.0) + subagents_usd),
    };
    // Kept beside a record too: they explain an estimate gap.
    let unpriced = sorted(
        std::iter::once(own)
            .chain(&c.parts)
            .filter_map(|p| p.unpriced_models.as_ref())
            .flatten()
            .cloned()
            .collect(),
    );
    let unreadable = c.unreadable;
    if total.is_none() && unpriced.is_none() && unreadable == 0 && native.is_none() {
        return None;
    }
    Some(SessionCost {
        from_agent: headline.is_some(),
        total_usd: total,
        subagents_usd: (subagents_usd > 0.0).then_some(subagents_usd),
        unreadable_subagents: (unreadable > 0).then_some(unreadable),
        recorded_usd: native.map(|n| n.usd),
        estimate_usd: estimate,
        record_partial: native.is_some_and(|n| n.partial),
        estimate_gap: headline
            .zip(estimate)
            .and_then(|(n, e)| estimate_gap(n.usd, e)),
        estimate_warning: headline
            .zip(estimate)
            .is_some_and(|(n, e)| estimate_warns(n.usd, e, unpriced.is_some())),
        unpriced_models: unpriced,
    })
}

/// [`cost_of`] a parsed session whose turns are priced.
pub fn session_cost(p: &ParsedSession, priced_turns: &[Turn]) -> Option<SessionCost> {
    cost_of(&CostParts {
        own: sum_priced(priced_turns),
        parts: p
            .sidechain_cost
            .iter()
            .chain(p.subagents.iter().map(|s: &SubagentRef| &s.cost))
            .cloned()
            .collect(),
        native: p.native_cost,
        unreadable: p.subagents.iter().filter(|s| s.unreadable).count(),
    })
}

/// The list row's cost: the headline [`cost_of`] gives the session detail.
/// Beside a record that already counts subagents, the subagent share stays
/// unknown, since the list does not read their files for it.
pub fn header_cost(c: &CostParts) -> HeaderCost {
    cost_of(c).map_or_else(HeaderCost::default, |s| HeaderCost {
        usd: s.total_usd,
        from_agent: s.from_agent,
        subagents_usd: s
            .subagents_usd
            .filter(|_| !(s.from_agent && c.native.is_some_and(|n| n.includes_subagents))),
        unpriced_models: s.unpriced_models.filter(|_| !s.from_agent),
    })
}

const CENT: f64 = 0.005;

/// An estimated USD figure: `~$1.23`, or `<$0.01` below half a cent.
pub fn fmt_usd(n: f64) -> String {
    if n > 0.0 && n < CENT {
        "<$0.01".to_owned()
    } else {
        format!("~${}", crate::js::num::to_fixed(n, 2))
    }
}

/// A recorded USD figure: `$1.23`, or `<$0.01` below half a cent.
pub fn fmt_recorded_usd(n: f64) -> String {
    if n > 0.0 && n < CENT {
        "<$0.01".to_owned()
    } else {
        format!("${}", crate::js::num::to_fixed(n, 2))
    }
}

/// A session cost as display text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostSummary {
    /// `—` when nothing is priced.
    pub total: String,
    pub notes: Vec<String>,
    /// Record and estimate disagree in a way that points at a price
    /// problem; shown as a warning.
    pub warning: Option<String>,
    /// The estimate beside a record when it does not warn; shown quietly.
    pub estimate_note: Option<String>,
}

/// The total and the notes that qualify it, led by where it comes from.
pub fn cost_summary(cost: &SessionCost, provider: Provider) -> CostSummary {
    let agent_label = provider.label();
    let mut notes = vec![if cost.from_agent {
        format!("recorded by {agent_label}")
    } else {
        "estimated from tokens".to_owned()
    }];
    if let Some(s) = cost.subagents_usd.filter(|s| *s != 0.0 && !s.is_nan()) {
        notes.push(format!("incl. subagents {}", fmt_usd(s)));
    }
    if let Some(r) = cost.recorded_usd.filter(|_| cost.record_partial) {
        notes.push(format!(
            "{agent_label} recorded {} for part of the session",
            fmt_recorded_usd(r)
        ));
    }
    if let Some(models) = cost.unpriced_models.as_ref().filter(|m| !m.is_empty()) {
        notes.push(format!("unpriced: {}", models.join(", ")));
    }
    if let Some(n) = cost.unreadable_subagents.filter(|n| *n > 0) {
        notes.push(format!("{n} subagent(s) unreadable"));
    }
    let (warning, estimate_note) = estimate_lines(cost, provider);
    let total = cost.total_usd.map_or_else(
        || "—".to_owned(),
        |t| {
            if cost.from_agent {
                fmt_recorded_usd(t)
            } else {
                fmt_usd(t)
            }
        },
    );
    CostSummary {
        total,
        notes,
        warning,
        estimate_note,
    }
}

/// `estimate from tokens ~$34.37 (93% lower)`.
pub fn estimate_warning(recorded: f64, estimate: f64) -> String {
    let g = gap(recorded, estimate).unwrap_or(0.0);
    let pct = crate::js::num::to_fixed((g * 100.0).abs(), 0);
    let dir = if g < 0.0 { "lower" } else { "higher" };
    format!("estimate from tokens {} ({pct}% {dir})", fmt_usd(estimate))
}

/// The quiet note for the estimate beside a full record. Only Claude Code
/// bills calls its transcript does not log, so only its note says so; the
/// others record the same calls at their own prices.
pub fn estimate_note(estimate: f64, provider: Provider) -> String {
    let usd = fmt_usd(estimate);
    match provider {
        Provider::ClaudeCode => {
            format!("Estimate from transcript: {usd}. Transcripts miss some billed calls.")
        }
        Provider::Codex | Provider::Pi | Provider::Opencode => {
            format!("Estimate at list price: {usd}.")
        }
    }
}

/// The warning or the quiet note for the estimate beside a full record;
/// both `None` without one.
pub fn estimate_lines(cost: &SessionCost, provider: Provider) -> (Option<String>, Option<String>) {
    match (cost.recorded_usd, cost.estimate_usd) {
        (Some(r), Some(e)) if cost.from_agent && !cost.record_partial => {
            if cost.estimate_warning {
                (Some(estimate_warning(r, e)), None)
            } else {
                (None, Some(estimate_note(e, provider)))
            }
        }
        _ => (None, None),
    }
}
