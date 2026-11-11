//! API list prices per model generation (USD per million tokens) and the
//! per-turn cost they give. Prices checked 2026-11-08; the table must match
//! `packages/core/src/services/pricing.ts`.

use std::sync::LazyLock;

use crate::js::text;

/// Rates in USD per million tokens. `cache_write` is the 5-minute (or only)
/// write rate; `cache_write_1h` falls back to it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub cache_write_1h: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provider {
    Anthropic,
    OpenAi,
}

/// A rate card, with the tier a long prompt bills at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceCard {
    rates: ModelPrice,
    long: Option<(f64, ModelPrice)>,
    provider: Provider,
}

/// Token counts one model call is priced from.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CostTokens {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_creation: f64,
    pub cache_creation_1h: Option<f64>,
}

const HAIKU_LONG_OVER: f64 = 100_000.0;
const OPENAI_LONG_OVER: f64 = 272_000.0;

const fn rates(
    input: f64,
    cache_write: f64,
    cache_write_1h: f64,
    cache_read: f64,
    output: f64,
) -> ModelPrice {
    ModelPrice {
        input,
        output,
        cache_read,
        cache_write,
        cache_write_1h: Some(cache_write_1h),
    }
}

const fn anthropic(r: ModelPrice) -> PriceCard {
    PriceCard {
        rates: r,
        long: None,
        provider: Provider::Anthropic,
    }
}

/// An `OpenAI` card: no listed write rate bills writes as input; the long tier
/// reuses the write rate.
const fn openai(
    input: f64,
    cached: f64,
    output: f64,
    write: Option<f64>,
    long: Option<(f64, f64, f64)>,
) -> PriceCard {
    let cache_write = match write {
        Some(w) => w,
        None => input,
    };
    let base = ModelPrice {
        input,
        output,
        cache_read: cached,
        cache_write,
        cache_write_1h: None,
    };
    let long = match long {
        Some((input, cache_read, output)) => Some((
            OPENAI_LONG_OVER,
            ModelPrice {
                input,
                output,
                cache_read,
                cache_write,
                cache_write_1h: None,
            },
        )),
        None => None,
    };
    PriceCard {
        rates: base,
        long,
        provider: Provider::OpenAi,
    }
}

const FABLE_5_1: PriceCard = anthropic(rates(10.0, 12.5, 20.0, 0.25, 50.0));
const FABLE_5: PriceCard = anthropic(rates(10.0, 12.5, 20.0, 1.0, 50.0));
const OPUS_5_5: PriceCard = anthropic(rates(4.0, 5.0, 8.0, 0.2, 20.0));
const OPUS_5: PriceCard = anthropic(rates(5.0, 6.25, 10.0, 0.5, 25.0));
const OPUS_4: PriceCard = anthropic(rates(15.0, 18.75, 30.0, 1.5, 75.0));
const SONNET_5_5: PriceCard = anthropic(rates(2.0, 2.5, 4.0, 0.1, 10.0));
const SONNET_5: PriceCard = anthropic(rates(2.0, 2.5, 4.0, 0.2, 10.0));
const SONNET_4: PriceCard = anthropic(rates(3.0, 3.75, 6.0, 0.3, 15.0));
const HAIKU_5_5: PriceCard = PriceCard {
    rates: rates(0.1, 0.125, 0.2, 0.01, 0.5),
    long: Some((HAIKU_LONG_OVER, rates(0.5, 0.625, 1.0, 0.05, 2.5))),
    provider: Provider::Anthropic,
};
const HAIKU_4_5: PriceCard = anthropic(rates(1.0, 1.25, 2.0, 0.1, 5.0));
const GPT_5_3: PriceCard = openai(1.75, 0.175, 14.0, None, None);
const GPT_5_1: PriceCard = openai(1.25, 0.125, 10.0, None, None);
const GPT_5_MINI: PriceCard = openai(0.25, 0.025, 2.0, None, None);
const GPT_4_1: PriceCard = openai(2.0, 0.5, 8.0, None, None);

/// Table order matters only among keys of equal length.
const CARDS: &[(&str, PriceCard)] = &[
    ("fable-5-1", FABLE_5_1),
    ("mythos-5-1", FABLE_5_1),
    ("fable-5", FABLE_5),
    ("mythos-5", FABLE_5),
    ("opus-5-5", OPUS_5_5),
    ("opus-5", OPUS_5),
    ("opus-4-8", OPUS_5),
    ("opus-4-7", OPUS_5),
    ("opus-4-6", OPUS_5),
    ("opus-4-5", OPUS_5),
    ("opus-4-1", OPUS_4),
    ("opus-4", OPUS_4),
    ("sonnet-5-5", SONNET_5_5),
    ("sonnet-5", SONNET_5),
    ("sonnet-4-6", SONNET_4),
    ("sonnet-4-5", SONNET_4),
    ("sonnet-4", SONNET_4),
    ("haiku-5-5", HAIKU_5_5),
    ("haiku-4-5", HAIKU_4_5),
    (
        "gpt-6-astra",
        openai(10.0, 1.0, 50.0, Some(12.5), Some((20.0, 2.0, 75.0))),
    ),
    (
        "gpt-6.1-sol",
        openai(2.0, 0.1, 10.0, Some(2.5), Some((4.0, 0.2, 15.0))),
    ),
    (
        "gpt-6.1-terra",
        openai(1.0, 0.05, 5.0, Some(1.25), Some((2.0, 0.1, 7.5))),
    ),
    (
        "gpt-6-sol",
        openai(2.0, 0.2, 10.0, Some(2.5), Some((4.0, 0.4, 15.0))),
    ),
    (
        "gpt-6-luna",
        openai(0.1, 0.01, 0.5, Some(0.125), Some((0.2, 0.02, 0.75))),
    ),
    (
        "gpt-5.6-sol",
        openai(4.0, 0.4, 20.0, Some(5.0), Some((8.0, 0.8, 30.0))),
    ),
    (
        "gpt-5.6-terra",
        openai(2.0, 0.2, 12.0, Some(2.5), Some((4.0, 0.4, 18.0))),
    ),
    (
        "gpt-5.6-luna",
        openai(0.2, 0.02, 1.2, Some(0.25), Some((0.4, 0.04, 1.8))),
    ),
    (
        "gpt-5.5",
        openai(5.0, 0.5, 30.0, None, Some((10.0, 1.0, 45.0))),
    ),
    (
        "gpt-5.4",
        openai(2.5, 0.25, 15.0, None, Some((5.0, 0.5, 22.5))),
    ),
    ("gpt-5.4-mini", openai(0.75, 0.075, 4.5, None, None)),
    ("gpt-5.4-nano", openai(0.2, 0.02, 1.25, None, None)),
    ("gpt-5.3-codex", GPT_5_3),
    ("gpt-5.2", GPT_5_3),
    ("gpt-5.2-codex", GPT_5_3),
    ("gpt-5.1", GPT_5_1),
    ("gpt-5.1-codex", GPT_5_1),
    ("gpt-5.1-codex-max", GPT_5_1),
    ("gpt-5.1-codex-mini", GPT_5_MINI),
    ("gpt-5", GPT_5_1),
    ("gpt-5-codex", GPT_5_1),
    ("gpt-5-mini", GPT_5_MINI),
    ("gpt-5-nano", openai(0.05, 0.005, 0.4, None, None)),
    ("codex-mini-latest", openai(1.5, 0.375, 6.0, None, None)),
    ("o3", GPT_4_1),
    ("o4-mini", openai(1.1, 0.275, 4.4, None, None)),
    ("gpt-4.1", GPT_4_1),
    ("gpt-4.1-mini", openai(0.4, 0.1, 1.6, None, None)),
];

/// Newest generation per Anthropic family, for a bare alias like `claude-opus`.
const FAMILY_FALLBACK: &[(&str, &str)] = &[
    ("opus", "opus-5-5"),
    ("sonnet", "sonnet-5-5"),
    ("haiku", "haiku-5-5"),
    ("fable", "fable-5-1"),
    ("mythos", "mythos-5-1"),
];

const fn is_boundary(b: u8) -> bool {
    matches!(b, b'/' | b'.' | b'-' | b'_' | b':')
}

/// `n` ASCII digits at the start of `s`, if present.
fn digits(s: &[u8], n: usize) -> Option<&[u8]> {
    let head = s.get(..n)?;
    head.iter().all(u8::is_ascii_digit).then(|| &s[n..])
}

/// Every remainder left after one `SUFFIX` match at the start of `s`.
fn one_suffix(s: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    if let Some(rest) = s.strip_prefix(b"-").and_then(|r| digits(r, 8)) {
        out.push(rest);
    }
    if let Some(rest) = s
        .strip_prefix(b"-")
        .and_then(|r| digits(r, 4))
        .and_then(|r| r.strip_prefix(b"-"))
        .and_then(|r| digits(r, 2))
        .and_then(|r| r.strip_prefix(b"-"))
        .and_then(|r| digits(r, 2))
    {
        out.push(rest);
    }
    if let Some(rest) = s.strip_prefix(b"@").and_then(|r| digits(r, 8)) {
        out.push(rest);
    }
    if let Some(rest) = s.strip_prefix(b"-v") {
        let run = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        for n in 1..=run {
            let after = &rest[n..];
            out.push(after);
            if let Some(colon) = after.strip_prefix(b":") {
                let run2 = colon.iter().take_while(|b| b.is_ascii_digit()).count();
                out.extend((1..=run2).map(|m| &colon[m..]));
            }
        }
    }
    if let Some(rest) = s.strip_prefix(b"-latest") {
        out.push(rest);
    }
    out
}

/// True when `s` is a run of date stamps, Bedrock versions and `-latest`
/// (`^(?:SUFFIX)*$`).
fn only_suffixes(s: &[u8]) -> bool {
    s.is_empty() || one_suffix(s).into_iter().any(only_suffixes)
}

/// True when `key` sits on a boundary in `id` and only allowed suffixes follow.
fn key_matches(id: &str, key: &str) -> bool {
    let bytes = id.as_bytes();
    let mut from = 0;
    while let Some(found) = id.get(from..).and_then(|rest| rest.find(key)) {
        let at = from + found;
        let before = at == 0 || is_boundary(bytes[at - 1]);
        if before && only_suffixes(&bytes[at + key.len()..]) {
            return true;
        }
        from = at + 1;
    }
    false
}

/// `dashed` with any trailing suffix run removed (`SUFFIX*$` replaced).
fn strip_trailing_suffixes(dashed: &str) -> &str {
    let bytes = dashed.as_bytes();
    (0..=bytes.len())
        .find(|&p| dashed.is_char_boundary(p) && only_suffixes(&bytes[p..]))
        .map_or(dashed, |p| &dashed[..p])
}

/// The family key a bare, version-free alias names.
fn alias_family(dashed: &str) -> Option<&'static str> {
    let stem = strip_trailing_suffixes(dashed);
    if stem.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    FAMILY_FALLBACK.iter().find_map(|&(family, key)| {
        let at = stem.len().checked_sub(family.len())?;
        let fits = stem.ends_with(family) && (at == 0 || is_boundary(stem.as_bytes()[at - 1]));
        fits.then_some(key)
    })
}

/// Cards sorted longest key first, so `opus-4-1` wins over `opus-4`.
static SORTED_CARDS: LazyLock<Vec<(&'static str, PriceCard)>> = LazyLock::new(|| {
    let mut cards = CARDS.to_vec();
    cards.sort_by_key(|(key, _)| std::cmp::Reverse(key.len()));
    cards
});

/// The card a model id resolves to: the most specific generation key, then a
/// bare family alias. `None` means unpriced.
pub fn price_for(model_id: &str) -> Option<(&'static str, PriceCard)> {
    let raw = text::trim(model_id).to_lowercase().replacen("[1m]", "", 1);
    let dashed = raw.replace('.', "-");
    SORTED_CARDS
        .iter()
        .find(|(key, card)| {
            let id = if card.provider == Provider::Anthropic {
                &dashed
            } else {
                &raw
            };
            key_matches(id, key)
        })
        .copied()
        .or_else(|| {
            let key = alias_family(&dashed)?;
            CARDS.iter().find(|(k, _)| *k == key).copied()
        })
}

/// USD for one token block at one rate card.
fn estimate_cost(t: &CostTokens, r: &ModelPrice) -> f64 {
    let one_hour = t.cache_creation_1h.unwrap_or(0.0).min(t.cache_creation);
    (t.input * r.input
        + t.output * r.output
        + t.cache_read * r.cache_read
        + (t.cache_creation - one_hour) * r.cache_write
        + one_hour * r.cache_write_1h.unwrap_or(r.cache_write))
        / 1e6
}

/// USD for one model call, `None` when the model is unpriced. The long tier
/// applies to the whole call when its prompt exceeds the card's threshold.
pub fn turn_cost(model: &str, t: &CostTokens) -> Option<f64> {
    if t.input == 0.0 && t.output == 0.0 && t.cache_read == 0.0 && t.cache_creation == 0.0 {
        return Some(0.0);
    }
    let (_, card) = price_for(model)?;
    let prompt = t.input + t.cache_read + t.cache_creation;
    let rates = match card.long {
        Some((over, long)) if prompt > over => long,
        _ => card.rates,
    };
    Some(estimate_cost(t, &rates))
}

#[cfg(test)]
mod tests {
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
    fn prices_a_turn() {
        let t = CostTokens {
            input: 1000.0,
            output: 50.0,
            cache_read: 2000.0,
            cache_creation: 3000.0,
            cache_creation_1h: Some(1000.0),
        };
        assert_eq!(turn_cost("claude-opus-5-5", &t), Some(0.0234));
        assert_eq!(turn_cost("mystery", &t), None);
        assert_eq!(turn_cost("mystery", &CostTokens::default()), Some(0.0));
    }

    #[test]
    fn long_tier() {
        let t = CostTokens {
            input: 100_001.0,
            ..CostTokens::default()
        };
        let cost = turn_cost("claude-haiku-5-5", &t).unwrap_or_default();
        assert!((cost - 100_001.0 * 0.5 / 1e6).abs() < 1e-12);
    }
}
