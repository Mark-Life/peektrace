//! Unit tests for `pricing.rs`.

use super::*;

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
fn the_most_specific_codex_key_wins() {
    let input = |id: &str| price_for(id).map(|(_, c)| c.rates.input);
    assert_eq!(input("gpt-5.1-codex"), Some(1.25));
    assert_eq!(input("gpt-5.1-codex-mini"), Some(0.25));
    assert_eq!(key("gpt-5.1-codex-max"), Some("gpt-5.1-codex-max"));
    assert_eq!(key("openai/gpt-5.5"), Some("gpt-5.5"));
}

#[test]
fn fast_mode_and_codex_spark_have_no_verified_rate() {
    assert_eq!(key("gpt-5.6-terra-fast"), None);
    assert_eq!(key("gpt-5.3-codex-spark"), None);
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

// TS `cost.test.ts`, Claude cases of the price table and turn cost.

fn input_rate(id: &str) -> Option<f64> {
    card_for(id).map(|c| c.rates.input)
}

#[test]
fn claude_generations_have_their_own_rates() {
    let opus = card_for("claude-opus-4-8").expect("priced");
    assert_eq!((opus.rates.input, opus.rates.output), (5.0, 25.0));
    assert_eq!(input_rate("claude-opus-4-1-20250805"), Some(15.0));
    assert_eq!(input_rate("claude-opus-4-20250514"), Some(15.0));
    assert_eq!(input_rate("claude-sonnet-5-5"), Some(2.0));
    assert_eq!(input_rate("claude-sonnet-5.5"), Some(2.0));
    assert!(card_for("claude-haiku-4-5-20251001").is_some());
}

#[test]
fn family_aliases_and_prefixes() {
    assert_eq!(key("claude-opus"), Some("opus-5-5"));
    assert_eq!(key("anthropic/claude-sonnet-latest"), Some("sonnet-5-5"));
    assert!(key("anthropic/claude-sonnet-4-6").is_some());
    assert_eq!(
        key("us.anthropic.claude-opus-4-1-20250805-v1:0"),
        Some("opus-4-1")
    );
    assert_eq!(key("claude-opus-4-8[1m]"), Some("opus-4-8"));
}

#[test]
fn unknown_versions_stay_unpriced() {
    for id in [
        "claude-opus-4-9",
        "claude-opus-4-1-thinking",
        "claude-sonnet-4-6-fast",
        "claude-3-opus-20240229",
        "claude-3-5-haiku-20241022",
        "claude-3-7-sonnet-latest",
    ] {
        assert_eq!(key(id), None, "{id}");
    }
}

fn cost(model: &str, t: &CostTokens) -> Option<f64> {
    turn_cost(card_for(model).as_ref(), t)
}

fn close(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-8)
}

#[test]
fn one_hour_writes_bill_at_the_one_hour_rate() {
    let split = CostTokens {
        cache_creation: 2_000_000.0,
        cache_creation_1h: Some(1_000_000.0),
        ..CostTokens::default()
    };
    assert!(close(cost("claude-sonnet-4-6", &split), 9.75));
    let plain = CostTokens {
        cache_creation: 1_000_000.0,
        ..CostTokens::default()
    };
    assert!(close(cost("claude-sonnet-4-6", &plain), 3.75));
}

#[test]
fn haiku_bills_the_whole_request_at_the_long_tier() {
    let input = |n: f64| CostTokens {
        input: n,
        ..CostTokens::default()
    };
    assert!(close(cost("claude-haiku-5-5", &input(99_000.0)), 0.0099));
    assert!(close(cost("claude-haiku-5-5", &input(101_000.0)), 0.0505));
}

#[test]
fn unknown_model_has_no_cost_and_zero_usage_costs_nothing() {
    let t = CostTokens {
        input: 10.0,
        ..CostTokens::default()
    };
    assert_eq!(cost("example-model-1", &t), None);
    assert_eq!(cost("<synthetic>", &CostTokens::default()), Some(0.0));
}

#[test]
fn an_unknown_id_is_unpriced_and_its_base_cost_is_zero() {
    assert!(card_for("<synthetic>").is_none());
    let t = CostTokens {
        input: 1e6,
        output: 1e6,
        ..CostTokens::default()
    };
    assert_eq!(base_cost(None, &t), 0.0);
}

#[test]
fn cache_reads_and_writes_bill_apart_from_input() {
    let card = card_for("claude-sonnet-4-6").expect("priced");
    let t = CostTokens {
        input: 1e6,
        cache_read: 1e6,
        cache_creation: 1e6,
        ..CostTokens::default()
    };
    let r = card.rates;
    assert!((base_cost(Some(&card), &t) - (r.input + r.cache_read + r.cache_write)).abs() < 5e-7);
}

#[test]
fn card_cache_matches_card_for() {
    let mut cache = CardCache::default();
    let ids = [
        "claude-opus-5-5",
        "gpt-5.1-codex",
        "nope-1",
        "claude-opus-5-5",
        "nope-1",
    ];
    for id in ids {
        assert_eq!(cache.get(id), card_for(id), "{id}");
    }
    assert_eq!(cache.0.len(), 3);
}
