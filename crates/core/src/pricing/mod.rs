//! API list prices per model (USD per million tokens) and the per-turn cost
//! they give. Rates come from a table generated from models.dev
//! (`models_dev.json`, rebuilt by `scripts/pricing-update.sh`), plus
//! hand-written overrides for what it does not list.

mod active;
mod matching;
mod overrides;
mod table;

use std::collections::HashMap;

use crate::js::text;
use matching::{alias_family, key_matches};
use table::Provider;

pub use active::{
    LOCAL_FILE, LocalTable, Origin, TableInfo, built_in_table, local_table_path, read_local_table,
    table_info, use_local_table,
};
pub use table::{MODELS_DEV_URL, Table, from_models_dev, parse_table, table_json};

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

/// The card a model id resolves to: the most specific generation key, then a
/// bare family alias. `None` means unpriced.
pub fn price_for(model_id: &str) -> Option<(&'static str, PriceCard)> {
    let raw = text::trim(model_id).to_lowercase().replacen("[1m]", "", 1);
    let dashed = raw.replace('.', "-");
    let cards = &active::active().cards;
    cards
        .iter()
        .find(|(key, card)| {
            let id = if card.provider == Provider::Anthropic {
                &dashed
            } else {
                &raw
            };
            key_matches(id, key)
        })
        .or_else(|| {
            let key = alias_family(&dashed)?;
            cards.iter().find(|(k, _)| k == key)
        })
        .map(|(k, card)| (k.as_str(), *card))
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

/// USD for a token block at a card's base rates (no long tier); an unpriced
/// model costs 0.
pub fn base_cost(card: Option<&PriceCard>, t: &CostTokens) -> f64 {
    card.map_or(0.0, |c| estimate_cost(t, &c.rates))
}

/// The cards `model_ids` resolve to, as `key in/out/write/read` rows plus the
/// table they come from and its date.
pub fn pricing_echo<'a>(model_ids: impl IntoIterator<Item = &'a str>) -> String {
    let mut rows: Vec<(&'static str, String)> = Vec::new();
    for id in model_ids {
        if let Some((key, card)) = price_for(id) {
            let r = card.rates;
            let row = [r.input, r.output, r.cache_write, r.cache_read]
                .map(crate::js::num::to_js_string)
                .join("/");
            match rows.iter_mut().find(|(k, _)| *k == key) {
                Some(found) => found.1 = row,
                None => rows.push((key, row)),
            }
        }
    }
    rows.sort_by(|a, b| a.0.cmp(b.0));
    let body = rows
        .iter()
        .map(|(k, r)| format!("{k} {r}"))
        .collect::<Vec<_>>()
        .join("; ");
    let body = if body.is_empty() {
        "no priced models".to_owned()
    } else {
        body
    };
    format!("{body} ({})", table_label(table_info()))
}

/// `models.dev 2026-11-16, built-in`: where prices come from and when they
/// were fetched.
pub fn table_label(info: &TableInfo) -> String {
    let source = if info.source == MODELS_DEV_URL {
        "models.dev"
    } else {
        info.source.as_str()
    };
    let origin = match info.origin {
        Origin::BuiltIn => "built-in",
        Origin::Downloaded => "downloaded",
    };
    format!("{source} {}, {origin}", info.fetched)
}

/// The rate card for `model_id`, `None` when it is unpriced.
pub fn card_for(model_id: &str) -> Option<PriceCard> {
    price_for(model_id).map(|(_, card)| card)
}

/// [`card_for`] memoised per model id: a corpus has many calls but few models.
#[derive(Debug, Default)]
pub struct CardCache(HashMap<String, Option<PriceCard>>);

impl CardCache {
    pub fn get(&mut self, model_id: &str) -> Option<PriceCard> {
        if let Some(card) = self.0.get(model_id) {
            return *card;
        }
        let card = card_for(model_id);
        self.0.insert(model_id.to_owned(), card);
        card
    }
}

/// USD for one model call at `card` (from [`card_for`]), `None` when the
/// model is unpriced. The long tier applies to the whole call when its prompt
/// exceeds the card's threshold.
pub fn turn_cost(card: Option<&PriceCard>, t: &CostTokens) -> Option<f64> {
    if t.input == 0.0 && t.output == 0.0 && t.cache_read == 0.0 && t.cache_creation == 0.0 {
        return Some(0.0);
    }
    let card = card?;
    let prompt = t.input + t.cache_read + t.cache_creation;
    let rates = match card.long {
        Some((over, long)) if prompt > over => long,
        _ => card.rates,
    };
    Some(estimate_cost(t, &rates))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod table_tests;
