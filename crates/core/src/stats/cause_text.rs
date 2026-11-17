//! The words and cells of one cause row: what its hits were, what its
//! retries cost in time and tokens, and who made them. Every surface prints
//! these, so they live in one place.

use peektrace_protocol::stats_text::{SUBAGENTS, YOU};

use super::detectors::retries::CauseRow;
use super::detectors::rules::Split;
use super::findings::FindingFormat;
use super::format::count;

/// `1 session`, `2 sessions`.
pub fn counted(n: f64, one: &str, many: &str, f: &FindingFormat) -> String {
    format!("{} {}", (f.count)(n), if n == 1.0 { one } else { many })
}

/// What a cause's hits are: shell commands, or calls of another tool.
const fn calls_word(row: &CauseRow) -> (&'static str, &'static str) {
    if row.shell {
        ("command", "commands")
    } else {
        ("call", "calls")
    }
}

/// `retry`, `retries`, or `read-back(s)` for a cause that is no failure.
pub const fn pairs_word(row: &CauseRow) -> (&'static str, &'static str) {
    if row.failing {
        ("retry", "retries")
    } else {
        ("read-back", "read-backs")
    }
}

/// `400 commands, all exited OK`, `120 commands, 30 exited OK`.
pub fn hits_text(row: &CauseRow, f: &FindingFormat) -> String {
    let exit0 = row.hits - row.flagged;
    let (one, many) = calls_word(row);
    let hits = counted(row.hits, one, many, f);
    if exit0 <= 0.0 {
        hits
    } else if exit0 >= row.hits {
        format!("{hits}, all exited OK")
    } else {
        format!("{hits}, {} exited OK", (f.count)(exit0))
    }
}

/// `~$26`, or `<$1` when it rounds to nothing.
pub fn approx_usd(usd: f64, f: &FindingFormat) -> String {
    peektrace_protocol::stats_text::approx_usd_text(usd, f.count)
}

/// `32m14s (2.8% of shell time)`.
pub fn time_share(sec: f64, share: f64, shell: bool, f: &FindingFormat) -> String {
    let of = if shell { "shell time" } else { "tool time" };
    format!("{} ({} of {of})", (f.hms)(sec), (f.pct)(share))
}

/// `17.4M tokens re-read (~$26)`.
pub fn reread_text(tokens: f64, usd: Option<f64>, f: &FindingFormat) -> String {
    let usd = usd.map_or_else(String::new, |u| format!(" ({})", approx_usd(u, f)));
    format!("{} tokens re-read{usd}", (f.tokens)(tokens))
}

/// Every split of one cause on one line: who hit it, how often per 1,000
/// calls, who retried, and what the retries re-read.
pub fn cause_split_with(row: &CauseRow, f: &FindingFormat) -> String {
    let side = |s: Split| {
        format!(
            "{YOU} {}, {SUBAGENTS} {}",
            (f.count)(s.main),
            (f.count)(s.nested)
        )
    };
    let rates = format!(
        "{YOU} {}, {SUBAGENTS} {}",
        per_thousand(row.per1k.main, row.hits_by.main),
        per_thousand(row.per1k.nested, row.hits_by.nested)
    );
    let what = if row.failing { "retries" } else { "read-backs" };
    let reread = if row.tokens_measured == 0.0 {
        "re-read not measured".to_owned()
    } else {
        reread_text(row.retry_ctx_tokens, row.retry_usd, f)
    };
    let exit0 = row.hits - row.flagged;
    let exit0 = if exit0 > 0.0 {
        format!(" · {} exited OK", (f.count)(exit0))
    } else {
        String::new()
    };
    format!(
        "hits: {} · per 1,000 calls: {rates} · {what}: {}{exit0} · {reread}",
        side(row.hits_by),
        side(row.pairs_by)
    )
}

/// One cause as one line, for a narrow screen.
pub fn cause_narrow_with(row: &CauseRow, f: &FindingFormat) -> String {
    let (one, many) = pairs_word(row);
    let sec = if row.sec_measured == 0.0 {
        String::new()
    } else {
        format!(
            " · {}",
            time_share(row.retry_sec, row.retry_share, row.shell, f)
        )
    };
    let toks = if row.tokens_measured == 0.0 {
        String::new()
    } else {
        format!(" · {}", reread_text(row.retry_ctx_tokens, row.retry_usd, f))
    };
    format!(
        "{} · {} in {}{sec}{toks}",
        counted(row.hits, "hit", "hits", f),
        counted(row.pairs, one, many, f),
        counted(row.retry_sessions, "session", "sessions", f)
    )
}

/// A per-1,000 rate with one decimal; a side with hits never reads `0`.
pub fn per_thousand(rate: f64, hits: f64) -> String {
    let fixed = crate::js::num::to_fixed(rate, 1);
    if hits == 0.0 {
        "0".to_owned()
    } else if fixed == "0.0" {
        "<0.1".to_owned()
    } else if rate >= 10.0 {
        count(rate.round())
    } else {
        fixed
    }
}

/// `32m14s · 2.8%`: a cause's retry time and its share, as a table cell.
pub fn time_cell(row: &CauseRow, f: &FindingFormat) -> String {
    if row.sec_measured == 0.0 {
        "—".to_owned()
    } else {
        format!("{} · {}", (f.hms)(row.retry_sec), (f.pct)(row.retry_share))
    }
}

/// `17.4M · ~$9`: re-read tokens and their approximate price, as a cell.
pub fn reread_cell(tokens: f64, usd: Option<f64>, measured: bool, f: &FindingFormat) -> String {
    if !measured {
        return "—".to_owned();
    }
    usd.map_or_else(
        || (f.tokens)(tokens),
        |u| format!("{} · {}", (f.tokens)(tokens), approx_usd(u, f)),
    )
}
