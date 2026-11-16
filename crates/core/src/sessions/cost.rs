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

/// A session's cost: the agent's own figure, else the sum of priced turns;
/// plus inline sidechain turns and every subagent. `None` when there is
/// nothing to price or report.
pub fn session_cost(p: &ParsedSession, priced_turns: &[Turn]) -> Option<SessionCost> {
    let own = sum_priced(priced_turns);
    let native = p.native_cost_usd;
    let main = native.or(own.cost_usd);
    let parts: Vec<&TurnsCost> = p
        .sidechain_cost
        .iter()
        .chain(p.subagents.iter().map(|s: &SubagentRef| &s.cost))
        .collect();
    let subagents_usd = parts.iter().fold(0.0, |n, c| n + c.cost_usd.unwrap_or(0.0));
    let mut unpriced: Vec<String> = if native.is_none() {
        own.unpriced_models.unwrap_or_default()
    } else {
        Vec::new()
    };
    unpriced.extend(
        parts
            .iter()
            .flat_map(|c| c.unpriced_models.iter().flatten().cloned()),
    );
    let unreadable = p.subagents.iter().filter(|s| s.unreadable).count();
    let total = if main.is_none() && !parts.iter().any(|c| c.cost_usd.is_some()) {
        None
    } else {
        Some(main.unwrap_or(0.0) + subagents_usd)
    };
    let unpriced = sorted(unpriced);
    if total.is_none() && unpriced.is_none() && unreadable == 0 {
        return None;
    }
    Some(SessionCost {
        from_agent: native.is_some(),
        total_usd: total,
        subagents_usd: (subagents_usd > 0.0).then_some(subagents_usd),
        unpriced_models: unpriced,
        unreadable_subagents: (unreadable > 0).then_some(unreadable),
    })
}
