//! The table in use: the built-in one, or a downloaded one that is newer.
//! Only a binary installs a downloaded table, at startup, so library callers
//! and tests always price from the built-in table.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::matching::strip_trailing_suffixes;
use super::overrides::{ONE_HOUR_WRITE_PER_INPUT, OVERRIDES};
use super::table::{Provider, Rates, Table, parse_table};
use super::{ModelPrice, PriceCard};

const BUILT_IN: &str = include_str!("models_dev.json");

/// The downloaded table's name in peektrace's data dir.
const LOCAL_FILE: &str = "prices.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableOrigin {
    BuiltIn,
    Downloaded,
}

/// Which table prices are read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableInfo {
    pub source: String,
    pub fetched: String,
    pub origin: TableOrigin,
    /// Models from the table, then hand-written overrides.
    pub models: usize,
    pub overrides: usize,
}

pub(super) struct Active {
    /// Longest key first, so `opus-4-1` wins over `opus-4`.
    pub cards: Vec<(String, PriceCard)>,
    pub info: TableInfo,
}

static ACTIVE: OnceLock<Active> = OnceLock::new();

/// The table compiled into this binary.
pub fn built_in_table() -> Result<Table, String> {
    parse_table(BUILT_IN)
}

fn price(r: Rates, provider: Provider) -> ModelPrice {
    ModelPrice {
        input: r.input,
        output: r.output,
        cache_read: r.cache_read,
        cache_write: r.cache_write,
        cache_write_1h: (provider == Provider::Anthropic)
            .then_some(r.input * ONE_HOUR_WRITE_PER_INPUT),
    }
}

fn card(provider: Provider, base: Rates, long: Option<(f64, Rates)>) -> PriceCard {
    PriceCard {
        rates: price(base, provider),
        long: long.map(|(over, r)| (over, price(r, provider))),
        provider,
    }
}

/// Anthropic keys drop `claude-` and read dots as dashes, as ids are matched.
fn key_of(provider: Provider, id: &str) -> String {
    let id = id.to_lowercase();
    match provider {
        Provider::Anthropic => id.strip_prefix("claude-").unwrap_or(&id).replace('.', "-"),
        Provider::OpenAi => id,
    }
}

/// Cards keyed overrides first, then each table in turn: the first source
/// with a key wins, so a downloaded table that dropped a model still leaves
/// the built-in price. A dated-only id is also keyed without its date, after
/// every exact key.
pub(super) fn resolve(tables: &[&Table], origin: TableOrigin) -> Active {
    let mut cards: Vec<(String, PriceCard)> = OVERRIDES
        .iter()
        .map(|&(p, key, r)| (key.to_owned(), card(p, r, None)))
        .collect();
    let keyed: Vec<(String, PriceCard)> = tables
        .iter()
        .flat_map(|t| &t.models)
        .map(|e| (key_of(e.provider, &e.id), card(e.provider, e.base, e.long)))
        .collect();
    let exact = keyed.iter().map(|(k, c)| (k.as_str(), *c));
    let undated = keyed.iter().map(|(k, c)| (strip_trailing_suffixes(k), *c));
    for (key, c) in exact.chain(undated) {
        if !cards.iter().any(|(k, _)| k == key) {
            cards.push((key.to_owned(), c));
        }
    }
    cards.sort_by_key(|(key, _)| std::cmp::Reverse(key.len()));
    let first = tables.first();
    Active {
        info: TableInfo {
            source: first.map(|t| t.source.clone()).unwrap_or_default(),
            fetched: first.map(|t| t.fetched.clone()).unwrap_or_default(),
            origin,
            models: first.map_or(0, |t| t.models.len()),
            overrides: OVERRIDES.len(),
        },
        cards,
    }
}

fn built_in_or_empty() -> Table {
    built_in_table().unwrap_or_else(|_| Table {
        source: String::new(),
        fetched: String::new(),
        models: Vec::new(),
    })
}

pub(super) fn active() -> &'static Active {
    ACTIVE.get_or_init(|| resolve(&[&built_in_or_empty()], TableOrigin::BuiltIn))
}

/// The table prices are read from.
pub fn table_info() -> &'static TableInfo {
    &active().info
}

/// `$PEEKTRACE_DIR/prices.json`, else `~/.peektrace/prices.json`.
pub fn local_table_path(peektrace_dir: Option<&str>, home: &str) -> PathBuf {
    match peektrace_dir.filter(|d| !d.is_empty()) {
        Some(dir) => Path::new(dir).join(LOCAL_FILE),
        None => Path::new(home).join(".peektrace").join(LOCAL_FILE),
    }
}

/// What a downloaded table file holds, against the built-in table.
#[derive(Debug, Clone, PartialEq)]
pub enum LocalTable {
    Missing,
    Invalid(String),
    /// Not newer than the built-in table, so not used.
    NotNewer(Table),
    Newer(Table),
}

/// Tomorrow in UTC, as `YYYY-MM-DD`: no clock is that far behind a real
/// download, so a later date is a bad file that would outrank every future
/// built-in table.
pub fn tomorrow() -> String {
    let today = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date();
    today.tomorrow().unwrap_or(today).to_string()
}

/// Read the downloaded table at `path`; nothing is installed.
pub fn read_local_table(path: &Path) -> LocalTable {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return LocalTable::Missing,
        Err(e) => return LocalTable::Invalid(e.to_string()),
    };
    let table = match parse_table(&text) {
        Ok(t) => t,
        Err(e) => return LocalTable::Invalid(e),
    };
    if table.fetched > tomorrow() {
        return LocalTable::Invalid(format!("dated {}, in the future", table.fetched));
    }
    let built_in = built_in_table().map(|t| t.fetched).unwrap_or_default();
    if table.fetched > built_in {
        LocalTable::Newer(table)
    } else {
        LocalTable::NotNewer(table)
    }
}

/// Price from the downloaded table at `path` when it is newer than the
/// built-in one; the built-in table still prices what it lacks. Call once at
/// startup, before any price lookup: the first lookup fixes the table for the
/// life of the process, so a later `pricing update` needs a restart. True
/// when it is in use.
pub fn use_local_table(path: &Path) -> bool {
    let LocalTable::Newer(table) = read_local_table(path) else {
        return false;
    };
    let set = ACTIVE
        .set(resolve(
            &[&table, &built_in_or_empty()],
            TableOrigin::Downloaded,
        ))
        .is_ok();
    debug_assert!(set, "a price lookup ran before the local table was set");
    set
}
