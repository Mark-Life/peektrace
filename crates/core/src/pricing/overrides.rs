//! Hand-written prices for what models.dev does not list or does not carry.
//! An override wins over a table entry with the same key.

use super::table::{Provider, Rates};

/// Anthropic bills a 1-hour cache write at twice the input rate; models.dev
/// lists only the 5-minute write.
pub const ONE_HOUR_WRITE_PER_INPUT: f64 = 2.0;

const fn r(input: f64, cache_write: f64, cache_read: f64, output: f64) -> Rates {
    Rates {
        input,
        output,
        cache_read,
        cache_write,
    }
}

const FABLE_5_1: Rates = r(10.0, 12.5, 0.25, 50.0);
const FABLE_5: Rates = r(10.0, 12.5, 1.0, 50.0);
const OPUS_4: Rates = r(15.0, 18.75, 1.5, 75.0);
const SONNET_4: Rates = r(3.0, 3.75, 0.3, 15.0);
/// `OpenAI` lists no write rate for these: a write bills as input.
const GPT_5_3: Rates = r(1.75, 1.75, 0.175, 14.0);
const GPT_5_1: Rates = r(1.25, 1.25, 0.125, 10.0);
const GPT_5_MINI: Rates = r(0.25, 0.25, 0.025, 2.0);

/// Ids models.dev does not list, keyed as the table keys them (Anthropic
/// without `claude-`).
pub const OVERRIDES: &[(Provider, &str, Rates)] = &[
    (Provider::Anthropic, "mythos-5-1", FABLE_5_1),
    (Provider::Anthropic, "mythos-5", FABLE_5),
    (Provider::Anthropic, "opus-4-1", OPUS_4),
    (Provider::Anthropic, "opus-4", OPUS_4),
    (Provider::Anthropic, "sonnet-4", SONNET_4),
    (Provider::OpenAi, "gpt-5.2-codex", GPT_5_3),
    (Provider::OpenAi, "gpt-5.1-codex", GPT_5_1),
    (Provider::OpenAi, "gpt-5.1-codex-max", GPT_5_1),
    (Provider::OpenAi, "gpt-5.1-codex-mini", GPT_5_MINI),
    (Provider::OpenAi, "gpt-5-codex", GPT_5_1),
    (
        Provider::OpenAi,
        "codex-mini-latest",
        r(1.5, 1.5, 0.375, 6.0),
    ),
];
