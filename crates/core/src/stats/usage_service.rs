//! `stats usage`: a widened pass over the usage rows, plus a full-history
//! pass when the first one may have cut a block chain.

use jiff::tz::{Offset, TimeZone};
use tracing::info_span;

use super::detectors::usage::{DAY_MS, UsageBy, UsageResult, UsageRow, UsageSink};
use super::pass::{PassArgs, PassEnv, run_pass, scope_of};
use super::service::{StatsEnv, StatsError};
use crate::js::json_out::Writer;
use crate::js::time::to_iso_string;
use crate::model::Agent;

pub const USAGE_SCHEMA_VERSION: &str = "peektrace-stats-usage/v1";
pub const MAX_USAGE_DAYS: i64 = 36_500;
const MONTH_WIDEN_DAYS: i64 = 32;

/// A zone name as `Intl.DateTimeFormat` accepts it: an IANA name or alias
/// (any case), or a fixed offset `±HH`, `±HHMM`, `±HH:MM`.
pub fn zone_of(name: &str) -> Option<TimeZone> {
    if let Some(off) = fixed_offset(name) {
        return Some(off);
    }
    if name.is_empty() || name.eq_ignore_ascii_case("z") {
        return None;
    }
    TimeZone::get(name).ok()
}

fn fixed_offset(name: &str) -> Option<TimeZone> {
    let (sign, rest) = match name.as_bytes().first()? {
        b'+' => (1, &name[1..]),
        b'-' => (-1, &name[1..]),
        _ => return None,
    };
    let digits: String = rest.chars().filter(|c| *c != ':').collect();
    let colon = rest.contains(':');
    let ok = rest.bytes().all(|b| b.is_ascii_digit() || b == b':')
        && match digits.len() {
            2 => !colon,
            4 => !colon || (rest.len() == 5 && rest.as_bytes()[2] == b':'),
            _ => false,
        };
    if !ok {
        return None;
    }
    let h: i32 = digits[..2].parse().ok()?;
    let m: i32 = digits.get(2..).filter(|s| !s.is_empty()).map_or(Some(0), |s| s.parse().ok())?;
    if h > 23 || m > 59 {
        return None;
    }
    let secs = sign * (h * 3600 + m * 60);
    Offset::from_seconds(secs).ok().map(TimeZone::fixed)
}

#[derive(Debug, Clone)]
pub struct UsageRequest {
    pub agents: Option<Vec<Agent>>,
    pub by: UsageBy,
    pub days: Option<i64>,
    pub force: bool,
    pub tz: Option<String>,
}

pub struct UsageReport {
    pub generated_at: f64,
    pub by: UsageBy,
    pub tz: String,
    pub days: i64,
    pub from_ms: i64,
    pub now_ms: i64,
    pub result: UsageResult,
}

fn write_row(w: &mut Writer, r: &UsageRow) {
    w.begin_obj();
    w.key("key").str(&r.key);
    r.totals.write_fields(w);
    match &r.block {
        Some(b) => {
            w.key("block").begin_obj();
            w.key("startIso").str(&b.start_iso);
            w.key("endIso").str(&b.end_iso);
            w.key("lastIso").str(&b.last_iso);
            w.key("active").bool(b.active);
            w.end_obj();
        }
        None => w.key("block").null(),
    }
    w.end_obj();
}

impl UsageReport {
    #[expect(clippy::cast_precision_loss)]
    pub fn to_json(&self, pretty: bool) -> String {
        let r = &self.result;
        let mut w = Writer::new(pretty);
        w.begin_obj();
        w.key("schemaVersion").str(USAGE_SCHEMA_VERSION);
        w.key("generatedAt").num(self.generated_at);
        w.key("by").str(self.by.as_str());
        w.key("tz").text(&self.tz);
        w.key("window").begin_obj();
        w.key("days").num(self.days as f64);
        w.key("fromIso").str(&to_iso_string(self.from_ms));
        w.key("toIso").str(&to_iso_string(self.now_ms));
        w.end_obj();
        w.key("rows").begin_arr();
        for row in &r.rows {
            w.elem();
            write_row(&mut w, row);
        }
        w.end_arr();
        w.key("totals").begin_obj();
        r.totals.write_fields(&mut w);
        w.end_obj();
        w.key("unpricedModels").texts(r.unpriced_models.iter().map(String::as_str));
        w.key("skipped").begin_obj();
        w.key("noTimestamp").num(r.skipped.no_timestamp);
        w.key("zeroTokens").num(r.skipped.zero_tokens);
        w.key("duplicates").num(r.skipped.duplicates);
        w.key("otherAgents").num(r.skipped.other_agents);
        w.end_obj();
        w.key("caveats").texts(r.caveats.iter().map(String::as_str));
        w.end_obj();
        w.finish()
    }
}

/// Tokens and estimated cost per day, month or 5-hour block.
#[expect(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn usage(env: &StatsEnv<'_>, req: &UsageRequest) -> Result<UsageReport, StatsError> {
    let _span = info_span!("Stats.usage", by = req.by.as_str()).entered();
    let tz_name = req.tz.clone().unwrap_or_else(|| "UTC".to_owned());
    let days = req.days.unwrap_or(req.by.default_days());
    if !(1..=MAX_USAGE_DAYS).contains(&days) {
        return Err(StatsError::Scan(format!(
            "days must be a whole number from 1 to {MAX_USAGE_DAYS}: {days}"
        )));
    }
    let tz = zone_of(&tz_name).ok_or_else(|| StatsError::Scan(format!("unknown time zone: {tz_name}")))?;
    let now = env.now_ms as i64;
    let from_ms = now - days * DAY_MS;
    let widen = if req.by == UsageBy::Month { MONTH_WIDEN_DAYS * DAY_MS } else { DAY_MS };
    let scope = scope_of(req.agents.as_deref());
    let pass_env = PassEnv {
        claude: env.claude,
        cache: &env.cache,
        local: env.local,
    };
    let run = |from: Option<i64>, force: bool| {
        let mut sink = UsageSink::new(req.by, days, from, now, tz.clone(), env.local);
        run_pass(
            &pass_env,
            &PassArgs {
                scope: &scope,
                force,
                from_ms: from.unwrap_or(0) as f64,
                to_ms: now as f64,
                only: None,
            },
            &mut |f| sink.add(f),
        );
        sink.finish()
    };
    let first = run(Some(from_ms - widen), req.force);
    let result = if first.chain_cut { run(None, false) } else { first };
    Ok(UsageReport {
        generated_at: env.now_ms,
        by: req.by,
        tz: tz_name,
        days,
        from_ms,
        now_ms: now,
        result,
    })
}
