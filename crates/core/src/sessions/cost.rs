//! Session cost from priced turns: a list-price estimate unless the agent
//! recorded its own figure.

use super::model::{ParsedSession, SessionCost, SubagentRef, Turn, TurnField, TurnsCost};
use crate::js::text;
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

/// Record and estimate differ past this share: a model may lack a price
/// card, or the transcript may miss calls.
pub const ESTIMATE_GAP_LIMIT: f64 = 0.05;

/// `(estimate - recorded) / recorded` when it passes the limit.
fn estimate_gap(recorded: f64, estimate: f64) -> Option<f64> {
    let gap = (estimate - recorded) / recorded;
    (recorded > 0.0 && gap.abs() > ESTIMATE_GAP_LIMIT).then_some(gap)
}

/// A session's cost. The agent's own record is the total when it covers the
/// whole session; a record that already counts subagents gets nothing added.
/// Otherwise the sum of priced turns, inline sidechain turns and subagents.
/// `None` when there is nothing to price or report.
pub fn session_cost(p: &ParsedSession, priced_turns: &[Turn]) -> Option<SessionCost> {
    let own = sum_priced(priced_turns);
    let parts: Vec<&TurnsCost> = p
        .sidechain_cost
        .iter()
        .chain(p.subagents.iter().map(|s: &SubagentRef| &s.cost))
        .collect();
    let subagents_usd = parts.iter().fold(0.0, |n, c| n + c.cost_usd.unwrap_or(0.0));
    let parts_priced = parts.iter().any(|c| c.cost_usd.is_some());
    let native = p.native_cost;
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
    // A main-only record prices every main turn; a whole-session record
    // keeps them, as they explain an estimate gap.
    let mut unpriced = if headline.is_some_and(|n| !n.includes_subagents) {
        Vec::new()
    } else {
        own.unpriced_models.unwrap_or_default()
    };
    unpriced.extend(
        parts
            .iter()
            .flat_map(|c| c.unpriced_models.iter().flatten().cloned()),
    );
    let unreadable = p.subagents.iter().filter(|s| s.unreadable).count();
    let unpriced = sorted(unpriced);
    if total.is_none() && unpriced.is_none() && unreadable == 0 && native.is_none() {
        return None;
    }
    Some(SessionCost {
        from_agent: headline.is_some(),
        total_usd: total,
        subagents_usd: (subagents_usd > 0.0).then_some(subagents_usd),
        unpriced_models: unpriced,
        unreadable_subagents: (unreadable > 0).then_some(unreadable),
        recorded_usd: native.map(|n| n.usd),
        estimate_usd: estimate,
        record_partial: native.is_some_and(|n| n.partial),
        estimate_gap: headline
            .zip(estimate)
            .and_then(|(n, e)| estimate_gap(n.usd, e)),
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
    /// Record and estimate disagree; shown as a warning.
    pub warning: Option<String>,
}

/// The total and the notes that qualify it, led by where it comes from.
pub fn cost_summary(cost: &SessionCost, agent_label: &str) -> CostSummary {
    let mut notes = vec![if cost.from_agent {
        format!("recorded by {agent_label}")
    } else {
        "estimated from tokens".to_owned()
    }];
    if let Some(s) = cost.subagents_usd.filter(|s| *s != 0.0 && !s.is_nan()) {
        notes.push(format!("subagents {}", fmt_usd(s)));
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
    let warning = cost
        .estimate_gap
        .zip(cost.estimate_usd)
        .map(|(gap, e)| estimate_warning(gap, e));
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
    }
}

/// `estimate from tokens ~$34.37 (93% lower)`.
pub fn estimate_warning(gap: f64, estimate: f64) -> String {
    let pct = crate::js::num::to_fixed((gap * 100.0).abs(), 0);
    let dir = if gap < 0.0 { "lower" } else { "higher" };
    format!("estimate from tokens {} ({pct}% {dir})", fmt_usd(estimate))
}
