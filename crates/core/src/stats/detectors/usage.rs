//! Usage: tokens and estimated USD per day, month or 5-hour block.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::js::json_out::Writer;
use crate::js::regex::LazyRe;
use crate::js::time::{LocalZone, parse_date, to_iso_string};
use crate::js_re;
use crate::model::Agent;
use crate::pricing::{CardCache, CostTokens, PriceCard, card_for, pricing_echo, turn_cost};
use crate::stats::rank::{cmp_str, round, uniq_sorted};
use crate::stats::schema::{Fact, UsageFact};

pub const HOUR_MS: i64 = 3_600_000;
pub const DAY_MS: i64 = 86_400_000;
pub const BLOCK_MS: i64 = 5 * HOUR_MS;
const SLOT_MS: i64 = 900_000;
static LINE_ID: LazyRe = js_re!(r"^line-\d+$");

/// How usage is bucketed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageBy {
    Day,
    Month,
    Block,
}

impl UsageBy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Month => "month",
            Self::Block => "block",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "day" => Some(Self::Day),
            "month" => Some(Self::Month),
            "block" => Some(Self::Block),
            _ => None,
        }
    }

    pub const fn default_days(self) -> i64 {
        match self {
            Self::Day => 30,
            Self::Month => 365,
            Self::Block => 7,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Agg {
    pub turns: f64,
    pub usd: f64,
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_creation: f64,
}

impl Agg {
    fn add(&mut self, t: &Self, turns: f64) {
        self.turns += turns;
        self.usd += t.usd;
        self.input += t.input;
        self.output += t.output;
        self.cache_read += t.cache_read;
        self.cache_creation += t.cache_creation;
    }

    fn total(&self) -> f64 {
        self.input + self.output + self.cache_read + self.cache_creation
    }
}

fn priced_of(f: &UsageFact, card: Option<&PriceCard>) -> Agg {
    let t = CostTokens {
        input: f.input_tokens,
        output: f.output_tokens,
        cache_read: f.cache_read_tokens,
        cache_creation: f.cache_creation_tokens,
        cache_creation_1h: None,
    };
    Agg {
        turns: 0.0,
        usd: turn_cost(card, &t).unwrap_or(0.0),
        input: t.input,
        output: t.output,
        cache_read: t.cache_read,
        cache_creation: t.cache_creation,
    }
}

/// The key two copies of one model call share.
pub fn dedup_key_of(agent: Agent, sid: &str, req: &str) -> String {
    if agent == Agent::Claude && !LINE_ID.is_match(req) {
        req.to_owned()
    } else {
        format!("{sid}\0{req}")
    }
}

/// `YYYY-MM-DD` of an instant in `tz`, memoised per 15-minute slot.
pub struct DayKey {
    tz: TimeZone,
    memo: HashMap<i64, String>,
}

impl DayKey {
    pub fn new(tz: TimeZone) -> Self {
        Self {
            tz,
            memo: HashMap::new(),
        }
    }

    pub fn of(&mut self, ms: i64) -> String {
        let slot = ms.div_euclid(SLOT_MS);
        if let Some(k) = self.memo.get(&slot) {
            return k.clone();
        }
        let key = Timestamp::from_millisecond(slot * SLOT_MS)
            .map(|t| {
                t.to_zoned(self.tz.clone())
                    .date()
                    .strftime("%Y-%m-%d")
                    .to_string()
            })
            .unwrap_or_default();
        self.memo.insert(slot, key.clone());
        key
    }
}

fn days_before(day: &str, n: i64) -> String {
    let mut it = day.split('-').map(|p| p.parse::<i64>().unwrap_or(0));
    let (y, m, d) = (
        it.next().unwrap_or(0),
        it.next().unwrap_or(1),
        it.next().unwrap_or(1),
    );
    let ms = crate::js::time::utc_ms(y, m - 1, d - n);
    to_iso_string(ms)[..10].to_owned()
}

#[derive(Debug, Clone)]
struct BlockCall {
    ms: i64,
    model: String,
    tokens: Agg,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageBlock {
    pub start_iso: String,
    pub end_iso: String,
    pub last_iso: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageTotals {
    pub agg: Agg,
    pub total_tokens: f64,
    pub models: Vec<String>,
}

impl UsageTotals {
    fn of(by_model: &IndexMap<String, Agg>) -> Self {
        let mut sum = Agg::default();
        for a in by_model.values() {
            sum.add(a, a.turns);
        }
        Self {
            total_tokens: sum.total(),
            agg: Agg {
                usd: round(sum.usd),
                ..sum
            },
            models: uniq_sorted(by_model.keys().map(String::as_str)),
        }
    }

    pub fn write_fields(&self, w: &mut Writer) {
        w.key("turns").num(self.agg.turns);
        w.key("usd").num(self.agg.usd);
        w.key("inputTokens").num(self.agg.input);
        w.key("outputTokens").num(self.agg.output);
        w.key("cacheReadTokens").num(self.agg.cache_read);
        w.key("cacheCreationTokens").num(self.agg.cache_creation);
        w.key("totalTokens").num(self.total_tokens);
        w.key("models")
            .texts(self.models.iter().map(String::as_str));
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageRow {
    pub key: String,
    pub totals: UsageTotals,
    pub block: Option<UsageBlock>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UsageSkipped {
    pub no_timestamp: f64,
    pub zero_tokens: f64,
    pub duplicates: f64,
    pub other_agents: f64,
}

pub struct UsageResult {
    pub caveats: Vec<String>,
    pub chain_cut: bool,
    pub rows: Vec<UsageRow>,
    pub skipped: UsageSkipped,
    pub totals: UsageTotals,
    pub unpriced_models: Vec<String>,
}

pub struct UsageSink<'a> {
    by: UsageBy,
    from_ms: Option<i64>,
    now_ms: i64,
    day_key: DayKey,
    cutoff_key: String,
    cutoff_ms: i64,
    buckets: IndexMap<String, IndexMap<String, Agg>>,
    calls: Vec<BlockCall>,
    seen: HashSet<String>,
    skipped: UsageSkipped,
    cards: CardCache,
    local: &'a dyn LocalZone,
}

fn fold(target: &mut IndexMap<String, Agg>, model: &str, t: &Agg) {
    target.entry(model.to_owned()).or_default().add(t, 1.0);
}

impl<'a> UsageSink<'a> {
    pub fn new(
        by: UsageBy,
        days: i64,
        from_ms: Option<i64>,
        now_ms: i64,
        tz: TimeZone,
        local: &'a dyn LocalZone,
    ) -> Self {
        let mut day_key = DayKey::new(tz);
        let cutoff_day = days_before(&day_key.of(now_ms), days - 1);
        let cutoff_key = if by == UsageBy::Month {
            cutoff_day[..7].to_owned()
        } else {
            cutoff_day
        };
        Self {
            by,
            from_ms,
            now_ms,
            day_key,
            cutoff_key,
            cutoff_ms: now_ms - days * DAY_MS,
            buckets: IndexMap::new(),
            calls: Vec::new(),
            seen: HashSet::new(),
            skipped: UsageSkipped::default(),
            cards: CardCache::default(),
            local,
        }
    }

    fn is_duplicate(&mut self, f: &UsageFact) -> bool {
        if self.seen.insert(dedup_key_of(f.agent, &f.sid, &f.req)) {
            false
        } else {
            self.skipped.duplicates += 1.0;
            true
        }
    }

    pub fn add(&mut self, fact: &Fact) {
        let Fact::Usage(f) = fact else { return };
        if f.input_tokens + f.output_tokens + f.cache_read_tokens + f.cache_creation_tokens == 0.0 {
            self.skipped.zero_tokens += 1.0;
            return;
        }
        let Some(ms) = f.ts.as_deref().and_then(|t| parse_date(t, self.local)) else {
            self.skipped.no_timestamp += 1.0;
            return;
        };
        if self.by == UsageBy::Block {
            if f.agent != Agent::Claude {
                self.skipped.other_agents += 1.0;
                return;
            }
            if !self.is_duplicate(f) {
                self.calls.push(BlockCall {
                    ms,
                    model: f.model.clone(),
                    tokens: priced_of(f, self.cards.get(&f.model).as_ref()),
                });
            }
            return;
        }
        let day = self.day_key.of(ms);
        let key = if self.by == UsageBy::Month {
            day[..7.min(day.len())].to_owned()
        } else {
            day
        };
        if cmp_str(&key, &self.cutoff_key).is_lt() || self.is_duplicate(f) {
            return;
        }
        let priced = priced_of(f, self.cards.get(&f.model).as_ref());
        fold(self.buckets.entry(key).or_default(), &f.model, &priced);
    }

    fn trusted_calls(&self) -> (Vec<BlockCall>, bool) {
        let Some(from) = self.from_ms else {
            return (self.calls.clone(), false);
        };
        let mut sorted = self.calls.clone();
        sorted.sort_by_key(|c| c.ms);
        let anchor = (0..sorted.len()).find(|&i| {
            let prev = if i == 0 { from } else { sorted[i - 1].ms };
            sorted[i].ms - prev > BLOCK_MS
        });
        match anchor {
            None => {
                let cut = !sorted.is_empty();
                (Vec::new(), cut)
            }
            Some(a) => {
                let cut = sorted[a].ms > self.cutoff_ms;
                (sorted.split_off(a), cut)
            }
        }
    }

    fn block_rows(
        &self,
        kept: &[BlockCall],
    ) -> Vec<(String, IndexMap<String, Agg>, Option<UsageBlock>)> {
        let mut sorted = kept.to_vec();
        sorted.sort_by_key(|c| c.ms);
        let mut open: Vec<(i64, i64, Vec<BlockCall>)> = Vec::new();
        for c in sorted {
            match open.last_mut() {
                Some(cur) if c.ms - cur.0 <= BLOCK_MS => {
                    cur.1 = c.ms;
                    cur.2.push(c);
                }
                _ => open.push((c.ms.div_euclid(HOUR_MS) * HOUR_MS, c.ms, vec![c])),
            }
        }
        open.into_iter()
            .filter(|(start, _, _)| start + BLOCK_MS > self.cutoff_ms)
            .map(|(start, last, calls)| {
                let mut by_model = IndexMap::new();
                for c in &calls {
                    fold(&mut by_model, &c.model, &c.tokens);
                }
                let block = UsageBlock {
                    start_iso: to_iso_string(start),
                    end_iso: to_iso_string(start + BLOCK_MS),
                    last_iso: to_iso_string(last),
                    active: self.now_ms < start + BLOCK_MS,
                };
                (block.start_iso.clone(), by_model, Some(block))
            })
            .collect()
    }

    fn caveats_of(&self, models: &[&str], unpriced: &[String]) -> Vec<String> {
        let s = &self.skipped;
        let mut out = vec![format!(
            "Cost is an ESTIMATE at API list prices, priced per call: a long prompt bills at its card's long tier, and every cache write at the 5-minute rate. Base rates per million tokens (input/output/cacheWrite/cacheRead USD): {}",
            pricing_echo(models.iter().copied())
        )];
        if !unpriced.is_empty() {
            out.push(format!(
                "Cost: {} model ids have no rate card and are left out of the cost — {}",
                unpriced.len(),
                unpriced.join(", ")
            ));
        }
        if s.duplicates > 0.0 {
            out.push(format!(
                "{} model calls repeat a request id already seen and count once, so totals can sit below the report's cost panel",
                s.duplicates
            ));
        }
        if s.no_timestamp > 0.0 {
            out.push(format!(
                "{} model calls carry no timestamp and are left out",
                s.no_timestamp
            ));
        }
        if self.by == UsageBy::Block {
            out.push("a block is the 5 hours from the UTC hour of its first call; a later call opens the next block".to_owned());
        }
        if s.other_agents > 0.0 {
            out.push(format!(
                "blocks cover Claude turns only: {} calls from other agents are left out",
                s.other_agents
            ));
        }
        out
    }

    pub fn finish(&self) -> UsageResult {
        let (built, chain_cut) = if self.by == UsageBy::Block {
            let (kept, cut) = self.trusted_calls();
            (self.block_rows(&kept), cut)
        } else {
            let mut rows: Vec<(String, IndexMap<String, Agg>, Option<UsageBlock>)> = self
                .buckets
                .iter()
                .map(|(k, v)| (k.clone(), v.clone(), None))
                .collect();
            rows.sort_by(|a, b| cmp_str(&a.0, &b.0));
            (rows, false)
        };
        let mut all: IndexMap<String, Agg> = IndexMap::new();
        for (_, by_model, _) in &built {
            for (m, a) in by_model {
                all.entry(m.clone()).or_default().add(a, a.turns);
            }
        }
        let unpriced = uniq_sorted(
            all.keys()
                .map(String::as_str)
                .filter(|m| card_for(m).is_none()),
        );
        let models: Vec<&str> = all.keys().map(String::as_str).collect();
        UsageResult {
            caveats: self.caveats_of(&models, &unpriced),
            chain_cut,
            rows: built
                .into_iter()
                .map(|(key, by_model, block)| UsageRow {
                    key,
                    totals: UsageTotals::of(&by_model),
                    block,
                })
                .collect(),
            skipped: self.skipped,
            totals: UsageTotals::of(&all),
            unpriced_models: unpriced,
        }
    }
}
