//! Model prices: id resolution and per-turn cost.

use peektrace_core::pricing::{CostTokens, base_cost, card_for, price_for, turn_cost};

fn key(id: &str) -> Option<&'static str> {
    price_for(id).map(|(k, _)| k)
}

#[test]
fn resolves_keys() {
    assert_eq!(key("claude-opus-5-5"), Some("opus-5-5"));
    assert_eq!(key("claude-opus-4-1-20250805"), Some("opus-4-1"));
    assert_eq!(key("claude-opus-4-20250514"), Some("opus-4"));
    assert_eq!(key("claude-sonnet-4.5[1m]"), Some("sonnet-4-5"));
    assert_eq!(
        key("anthropic.claude-haiku-4-5-20251001-v1:0"),
        Some("haiku-4-5")
    );
    assert_eq!(key("claude-opus-4-5@20251101"), Some("opus-4-5"));
    assert_eq!(key("claude-opus-latest"), Some("opus-5-5"));
    assert_eq!(key("sonnet"), Some("sonnet-5-5"));
    assert_eq!(key("gpt-5.1-codex-mini"), Some("gpt-5.1-codex-mini"));
    assert_eq!(key("gpt-5-1"), None);
    assert_eq!(key("claude-3-opus"), None);
    assert_eq!(key("claude-opus-4-1-thinking"), None);
    assert_eq!(key("mystery-model"), None);
    assert_eq!(key("<synthetic>"), None);
    assert_eq!(key("unknown"), None);
    assert_eq!(key("xopus-5-5"), None);
    assert_eq!(key("claude-opus-5-5-2026-01-01-latest"), Some("opus-5-5"));
}

#[test]
fn prices_a_turn() {
    let t = CostTokens {
        input: 1000.0,
        output: 50.0,
        cache_read: 2000.0,
        cache_creation: 3000.0,
        cache_creation_1h: Some(1000.0),
    };
    let opus = card_for("claude-opus-5-5");
    assert_eq!(turn_cost(opus.as_ref(), &t), Some(0.0234));
    assert_eq!(turn_cost(card_for("mystery").as_ref(), &t), None);
    assert_eq!(turn_cost(None, &CostTokens::default()), Some(0.0));
}

#[test]
fn long_tier() {
    let t = CostTokens {
        input: 100_001.0,
        ..CostTokens::default()
    };
    let cost = turn_cost(card_for("claude-haiku-5-5").as_ref(), &t).unwrap_or_default();
    assert!((cost - 100_001.0 * 0.5 / 1e6).abs() < 1e-12);
}

#[test]
fn an_unknown_id_is_unpriced_and_costs_nothing() {
    assert!(card_for("<synthetic>").is_none());
    let t = CostTokens {
        input: 1e6,
        output: 1e6,
        ..CostTokens::default()
    };
    assert_eq!(base_cost(None, &t), 0.0);
}
