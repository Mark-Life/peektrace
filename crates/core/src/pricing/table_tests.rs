//! The generated table against the hand-written cards it replaced, and the
//! table's own rules: conversion, parsing, layering and the local file.

use super::active::{built_in_table, read_local_table, resolve};
use super::table::{Rates, from_models_dev, parse_table, table_json};
use super::*;

const BUILT_IN: &str = include_str!("models_dev.json");

/// The hand-written cards before the table: key, input, 5m write, read,
/// output, and the long tier (over, input, write, read, output).
type OldCard = (&'static str, [f64; 4], Option<(f64, [f64; 4])>);

const fn a(key: &'static str, input: f64, write: f64, read: f64, output: f64) -> OldCard {
    (key, [input, write, read, output], None)
}

const OLD_ANTHROPIC: &[OldCard] = &[
    a("fable-5-2", 8.0, 10.0, 0.2, 40.0),
    a("fable-5-1", 10.0, 12.5, 0.25, 50.0),
    a("mythos-5-1", 10.0, 12.5, 0.25, 50.0),
    a("fable-5", 10.0, 12.5, 1.0, 50.0),
    a("mythos-5", 10.0, 12.5, 1.0, 50.0),
    a("opus-5-5", 4.0, 5.0, 0.2, 20.0),
    a("opus-5", 5.0, 6.25, 0.5, 25.0),
    a("opus-4-8", 5.0, 6.25, 0.5, 25.0),
    a("opus-4-7", 5.0, 6.25, 0.5, 25.0),
    a("opus-4-6", 5.0, 6.25, 0.5, 25.0),
    a("opus-4-5", 5.0, 6.25, 0.5, 25.0),
    a("opus-4-1", 15.0, 18.75, 1.5, 75.0),
    a("opus-4", 15.0, 18.75, 1.5, 75.0),
    a("sonnet-5-5", 2.0, 2.5, 0.1, 10.0),
    a("sonnet-5", 2.0, 2.5, 0.2, 10.0),
    a("sonnet-4-6", 3.0, 3.75, 0.3, 15.0),
    a("sonnet-4-5", 3.0, 3.75, 0.3, 15.0),
    a("sonnet-4", 3.0, 3.75, 0.3, 15.0),
    (
        "haiku-5-5",
        [0.1, 0.125, 0.01, 0.5],
        Some((100_000.0, [0.5, 0.625, 0.05, 2.5])),
    ),
    a("haiku-4-5", 1.0, 1.25, 0.1, 5.0),
];

const fn long(key: &'static str, base: [f64; 4], tier: [f64; 4]) -> OldCard {
    (key, base, Some((272_000.0, tier)))
}

/// `OpenAI` cards wrote a missing write rate as input, and reused the base
/// write rate in the long tier.
const OLD_OPENAI: &[OldCard] = &[
    long(
        "gpt-6-astra",
        [10.0, 12.5, 1.0, 50.0],
        [20.0, 12.5, 2.0, 75.0],
    ),
    long("gpt-6.1-sol", [2.0, 2.5, 0.1, 10.0], [4.0, 2.5, 0.2, 15.0]),
    long(
        "gpt-6.1-terra",
        [1.0, 1.25, 0.05, 5.0],
        [2.0, 1.25, 0.1, 7.5],
    ),
    long("gpt-6-sol", [2.0, 2.5, 0.2, 10.0], [4.0, 2.5, 0.4, 15.0]),
    long(
        "gpt-6-luna",
        [0.1, 0.125, 0.01, 0.5],
        [0.2, 0.125, 0.02, 0.75],
    ),
    long("gpt-5.6-sol", [4.0, 5.0, 0.4, 20.0], [8.0, 5.0, 0.8, 30.0]),
    long(
        "gpt-5.6-terra",
        [2.0, 2.5, 0.2, 12.0],
        [4.0, 2.5, 0.4, 18.0],
    ),
    long(
        "gpt-5.6-luna",
        [0.2, 0.25, 0.02, 1.2],
        [0.4, 0.25, 0.04, 1.8],
    ),
    long("gpt-5.5", [5.0, 5.0, 0.5, 30.0], [10.0, 5.0, 1.0, 45.0]),
    long("gpt-5.4", [2.5, 2.5, 0.25, 15.0], [5.0, 2.5, 0.5, 22.5]),
    a("gpt-5.4-mini", 0.75, 0.75, 0.075, 4.5),
    a("gpt-5.4-nano", 0.2, 0.2, 0.02, 1.25),
    a("gpt-5.3-codex", 1.75, 1.75, 0.175, 14.0),
    a("gpt-5.2", 1.75, 1.75, 0.175, 14.0),
    a("gpt-5.2-codex", 1.75, 1.75, 0.175, 14.0),
    a("gpt-5.1", 1.25, 1.25, 0.125, 10.0),
    a("gpt-5.1-codex", 1.25, 1.25, 0.125, 10.0),
    a("gpt-5.1-codex-max", 1.25, 1.25, 0.125, 10.0),
    a("gpt-5.1-codex-mini", 0.25, 0.25, 0.025, 2.0),
    a("gpt-5", 1.25, 1.25, 0.125, 10.0),
    a("gpt-5-codex", 1.25, 1.25, 0.125, 10.0),
    a("gpt-5-mini", 0.25, 0.25, 0.025, 2.0),
    a("gpt-5-nano", 0.05, 0.05, 0.005, 0.4),
    a("codex-mini-latest", 1.5, 1.5, 0.375, 6.0),
    a("o3", 2.0, 2.0, 0.5, 8.0),
    a("o4-mini", 1.1, 1.1, 0.275, 4.4),
    a("gpt-4.1", 2.0, 2.0, 0.5, 8.0),
    a("gpt-4.1-mini", 0.4, 0.4, 0.1, 1.6),
];

/// Where models.dev differs from the old cards: it lists a long-tier write
/// rate (twice the base one) that the old cards did not have.
const LONG_WRITE_DIFFERS: &[(&str, f64)] = &[
    ("gpt-6-astra", 25.0),
    ("gpt-6.1-sol", 5.0),
    ("gpt-6.1-terra", 2.5),
    ("gpt-6-sol", 5.0),
    ("gpt-6-luna", 0.25),
    ("gpt-5.6-sol", 10.0),
    ("gpt-5.6-terra", 5.0),
    ("gpt-5.6-luna", 0.5),
];

fn price(r: [f64; 4], one_hour: bool) -> ModelPrice {
    let [input, cache_write, cache_read, output] = r;
    ModelPrice {
        input,
        output,
        cache_read,
        cache_write,
        cache_write_1h: one_hour.then_some(input * 2.0),
    }
}

#[test]
fn the_table_covers_every_old_card_at_the_same_rates() {
    let cases = OLD_ANTHROPIC
        .iter()
        .map(|c| (format!("claude-{}", c.0), c, true))
        .chain(OLD_OPENAI.iter().map(|c| (c.0.to_owned(), c, false)));
    for (id, &(key, base, old_long), anthropic) in cases {
        let (got_key, card) = price_for(&id).unwrap_or_else(|| panic!("{id} is unpriced"));
        assert_eq!(got_key, key, "{id}");
        assert_eq!(card.rates, price(base, anthropic), "{id} base");
        let mut want = old_long.map(|(over, r)| (over, price(r, anthropic)));
        if let (Some((_, l)), Some((_, w))) =
            (&mut want, LONG_WRITE_DIFFERS.iter().find(|d| d.0 == key))
        {
            l.cache_write = *w;
        }
        assert_eq!(card.long, want, "{id} long tier");
    }
}

#[test]
fn the_built_in_table_parses_and_is_written_as_generated() {
    let table = built_in_table().expect("built-in table parses");
    assert!(table.models.len() > 40);
    assert_eq!(table_json(&table), BUILT_IN);
    assert_eq!(table_info().origin, TableOrigin::BuiltIn);
    assert_eq!(table_info().fetched, table.fetched);
}

const API: &str = r#"{
  "anthropic": {"models": {
    "claude-new-9": {"cost": {"input": 2, "output": 10, "cache_read": 0.2, "cache_write": 2.5}},
    "claude-new-9-20990101": {"cost": {"input": 2, "output": 10, "cache_read": 0.2, "cache_write": 2.5}},
    "claude-dated-1-20990101": {"cost": {"input": 1, "output": 5}},
    "claude-free": {"cost": null}
  }},
  "openai": {"models": {
    "gpt-x": {"cost": {"input": 1, "output": 4, "cache_read": 0.1,
      "tiers": [
        {"input": 3, "output": 9, "tier": {"type": "context", "size": 500000}},
        {"input": 2, "output": 8, "cache_read": 0.2, "tier": {"type": "context", "size": 200000}}
      ]}},
    "text-embedding-x": {"type": "embedding", "cost": {"input": 0.1, "output": 0}}
  }},
  "elsewhere": {"models": {"other-1": {"cost": {"input": 1, "output": 1}}}}
}"#;

#[test]
fn converts_models_dev() {
    let t = from_models_dev(API, "2099-01-02").expect("converts");
    let ids: Vec<&str> = t.models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["claude-dated-1-20990101", "claude-new-9", "gpt-x"]);
    let dated = &t.models[0];
    assert_eq!(
        dated.base.cache_read, 1.0,
        "a missing read rate bills as input"
    );
    let gpt = &t.models[2];
    let (over, l) = gpt.long.expect("smallest tier");
    assert_eq!(over, 200_000.0);
    assert_eq!((l.input, l.cache_read, l.cache_write), (2.0, 0.2, 1.0));
    assert_eq!(parse_table(&table_json(&t)), Ok(t));
}

#[test]
fn refuses_a_table_missing_a_provider() {
    assert!(from_models_dev(r#"{"openai":{"models":{}}}"#, "2099-01-01").is_err());
    assert!(from_models_dev("not json", "2099-01-01").is_err());
}

fn entry(provider: table::Provider, id: &str, input: f64) -> table::TableEntry {
    table::TableEntry {
        provider,
        id: id.to_owned(),
        base: Rates {
            input,
            output: input,
            cache_read: input,
            cache_write: input,
        },
        long: None,
    }
}

fn input_of(cards: &[(String, PriceCard)], key: &str) -> Option<f64> {
    cards
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, c)| c.rates.input)
}

#[test]
fn overrides_win_then_the_download_then_the_built_in_table() {
    use table::Provider::{Anthropic, OpenAi};
    let download = Table {
        source: MODELS_DEV_URL.to_owned(),
        fetched: "2099-01-01".to_owned(),
        models: vec![
            entry(Anthropic, "claude-opus-5-5", 99.0),
            entry(Anthropic, "claude-opus-4-1", 99.0),
            entry(OpenAi, "gpt-only-1-20990101", 7.0),
            entry(OpenAi, "gpt-4o-2024-05-13", 9.0),
        ],
    };
    let built_in = built_in_table().expect("parses");
    let active = resolve(&[&download, &built_in], TableOrigin::Downloaded);
    assert_eq!(input_of(&active.cards, "opus-5-5"), Some(99.0));
    assert_eq!(input_of(&active.cards, "opus-4-1"), Some(15.0), "override");
    assert_eq!(input_of(&active.cards, "sonnet-5-5"), Some(2.0), "built-in");
    assert_eq!(
        input_of(&active.cards, "gpt-only-1"),
        Some(7.0),
        "undated key"
    );
    assert_eq!(active.info.fetched, "2099-01-01");
}

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("peektrace-pricing-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn reads_the_local_table() {
    let dir = TempDir::new("local");
    let path = local_table_path(Some(&dir.0.to_string_lossy()), "/nowhere");
    assert_eq!(read_local_table(&path), LocalTable::Missing);
    std::fs::write(&path, "{").expect("write");
    assert!(matches!(read_local_table(&path), LocalTable::Invalid(_)));
    let mut table = built_in_table().expect("parses");
    std::fs::write(&path, table_json(&table)).expect("write");
    assert!(matches!(read_local_table(&path), LocalTable::NotNewer(_)));
    table.fetched = "2999-01-01".to_owned();
    std::fs::write(&path, table_json(&table)).expect("write");
    assert!(matches!(read_local_table(&path), LocalTable::Newer(_)));
    let other_format = table_json(&table).replace("\"format\": 1", "\"format\": 2");
    std::fs::write(&path, other_format).expect("write");
    assert!(matches!(read_local_table(&path), LocalTable::Invalid(_)));
    assert_eq!(
        local_table_path(None, "/home/u"),
        std::path::Path::new("/home/u/.peektrace/prices.json")
    );
}

#[test]
fn the_echo_names_the_table_and_its_date() {
    let echo = pricing_echo(["claude-opus-5-5"]);
    let date = &table_info().fetched;
    assert_eq!(
        echo,
        format!("opus-5-5 4/20/5/0.2 (models.dev {date}, built-in)")
    );
}
