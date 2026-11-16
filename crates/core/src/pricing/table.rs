//! The price table: its JSON form, and how it is built from models.dev's
//! `api.json`. One model per line, so a regenerated table diffs cleanly.

use serde_json::{Map, Value, json};

use super::matching::strip_trailing_suffixes;

pub const MODELS_DEV_URL: &str = "https://models.dev/api.json";

/// Bumped when the conversion rules change, so a table written by another
/// version is not read.
const TABLE_FORMAT: f64 = 1.0;

/// models.dev providers the table keeps: the agents we estimate for bill
/// through these.
const PROVIDERS: &[(&str, Provider)] = &[
    ("anthropic", Provider::Anthropic),
    ("openai", Provider::OpenAi),
];

/// Anthropic ids match with dots read as dashes; others match as written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    OpenAi,
}

impl Provider {
    const fn name(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        PROVIDERS.iter().find(|(n, _)| *n == name).map(|(_, p)| *p)
    }
}

/// USD per million tokens; `cache_write` is the 5-minute (or only) rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rates {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

/// One model: base rates, and the rates a prompt over `long.0` tokens bills at.
#[derive(Debug, Clone, PartialEq)]
pub struct TableEntry {
    pub provider: Provider,
    pub id: String,
    pub base: Rates,
    pub long: Option<(f64, Rates)>,
}

/// A price table and the day its data was fetched (`YYYY-MM-DD`).
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub source: String,
    pub fetched: String,
    pub models: Vec<TableEntry>,
}

fn num(o: &Map<String, Value>, k: &str) -> Option<f64> {
    o.get(k)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0)
}

/// Rates from a cost object. A missing read rate bills as input; a missing
/// write rate bills at `write_or`, else as input.
fn rates_of(
    o: &Map<String, Value>,
    read: &str,
    write: &str,
    write_or: Option<f64>,
) -> Option<Rates> {
    let input = num(o, "input")?;
    Some(Rates {
        input,
        output: num(o, "output")?,
        cache_read: num(o, read).unwrap_or(input),
        cache_write: num(o, write).or(write_or).unwrap_or(input),
    })
}

/// A real calendar day written `YYYY-MM-DD`.
fn is_date(s: &str) -> bool {
    s.parse::<jiff::civil::Date>()
        .is_ok_and(|d| d.to_string() == s)
}

/// Parse a table written by [`table_json`].
pub fn parse_table(text: &str) -> Result<Table, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let o = v.as_object().ok_or("not a JSON object")?;
    let str_of = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_owned);
    if o.get("format").and_then(Value::as_f64) != Some(TABLE_FORMAT) {
        return Err(format!("not a format {TABLE_FORMAT} price table"));
    }
    let fetched = str_of("fetched")
        .filter(|d| is_date(d))
        .ok_or("missing or bad `fetched` date")?;
    let models = o
        .get("models")
        .and_then(Value::as_array)
        .ok_or("missing `models`")?;
    let models = models
        .iter()
        .filter_map(|m| {
            let m = m.as_object()?;
            let provider = Provider::parse(m.get("provider")?.as_str()?)?;
            let base = rates_of(m, "cacheRead", "cacheWrite", None)?;
            let long = m.get("long").and_then(Value::as_object).and_then(|l| {
                Some((
                    num(l, "over")?,
                    rates_of(l, "cacheRead", "cacheWrite", None)?,
                ))
            });
            Some(TableEntry {
                provider,
                id: m.get("id")?.as_str()?.to_owned(),
                base,
                long,
            })
        })
        .collect::<Vec<_>>();
    if models.is_empty() {
        return Err("no models".to_owned());
    }
    Ok(Table {
        source: str_of("source").unwrap_or_default(),
        fetched,
        models,
    })
}

fn n(v: f64) -> String {
    crate::js::num::to_js_string(v)
}

fn rates_json(r: &Rates) -> String {
    format!(
        "\"input\":{},\"output\":{},\"cacheRead\":{},\"cacheWrite\":{}",
        n(r.input),
        n(r.output),
        n(r.cache_read),
        n(r.cache_write)
    )
}

/// The table as JSON text, one model per line, keys in a fixed order.
pub fn table_json(t: &Table) -> String {
    let lines = t
        .models
        .iter()
        .map(|e| {
            let long = e.long.as_ref().map_or_else(String::new, |(over, r)| {
                format!(",\"long\":{{\"over\":{},{}}}", n(*over), rates_json(r))
            });
            format!(
                "    {{\"provider\":{},\"id\":{},{}{long}}}",
                json!(e.provider.name()),
                json!(e.id),
                rates_json(&e.base)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        "{{\n  \"format\": {},\n  \"source\": {},\n  \"fetched\": {},\n  \"models\": [\n{lines}\n  ]\n}}\n",
        n(TABLE_FORMAT),
        json!(t.source),
        json!(t.fetched)
    )
}

/// The smallest context tier: a prompt above its size bills at its rates. A
/// tier with no write rate keeps the base one.
fn long_tier(cost: &Map<String, Value>, base: &Rates) -> Option<(f64, Rates)> {
    cost.get("tiers")?
        .as_array()?
        .iter()
        .filter_map(|t| {
            let t = t.as_object()?;
            let tier = t.get("tier")?.as_object()?;
            (tier.get("type")?.as_str()? == "context").then_some(())?;
            let rates = rates_of(t, "cache_read", "cache_write", Some(base.cache_write))?;
            Some((num(tier, "size")?, rates))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// Chat models of one provider; `type` marks embedding, image and realtime
/// models, which agents do not call.
fn provider_models(api: &Value, name: &str, provider: Provider) -> Vec<TableEntry> {
    let Some(models) = api
        .get(name)
        .and_then(|p| p.get("models"))
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    models
        .iter()
        .filter_map(|(id, m)| {
            let m = m.as_object()?;
            if m.contains_key("type") {
                return None;
            }
            let cost = m.get("cost")?.as_object()?;
            let base = rates_of(cost, "cache_read", "cache_write", None)?;
            Some(TableEntry {
                provider,
                id: id.clone(),
                long: long_tier(cost, &base),
                base,
            })
        })
        .collect()
}

/// True when `e` is another listed id plus a date or version suffix, at the
/// same rates: the shorter id already prices it.
fn is_dated_copy(e: &TableEntry, all: &[TableEntry]) -> bool {
    let stem = strip_trailing_suffixes(&e.id);
    stem != e.id
        && all.iter().any(|o| {
            o.provider == e.provider && o.id == stem && o.base == e.base && o.long == e.long
        })
}

/// Build a table from models.dev's `api.json` text.
pub fn from_models_dev(api_json: &str, fetched: &str) -> Result<Table, String> {
    let api: Value = serde_json::from_str(api_json).map_err(|e| e.to_string())?;
    let mut all = Vec::new();
    for (name, p) in PROVIDERS {
        let models = provider_models(&api, name, *p);
        if models.is_empty() {
            return Err(format!("no priced {name} models"));
        }
        all.extend(models);
    }
    let keep: Vec<bool> = all.iter().map(|e| !is_dated_copy(e, &all)).collect();
    let mut keep = keep.into_iter();
    all.retain(|_| keep.next().unwrap_or(true));
    all.sort_by(|a, b| (a.provider.name(), &a.id).cmp(&(b.provider.name(), &b.id)));
    Ok(Table {
        source: MODELS_DEV_URL.to_owned(),
        fetched: fetched.to_owned(),
        models: all,
    })
}
