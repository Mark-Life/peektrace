//! The text of each stats row and panel, and the rank bar geometry, as the
//! inspector's stats components write them.

use peektrace_protocol::stats::{
    CauseRowView, ClusterRowView, CorpusView, StatsReportView, TimeRowView, WaitPanelView,
};
use peektrace_protocol::{AgentId, DrillTable};

use peektrace_protocol::stats_text::{approx_usd_text, wait_shape_label};

use super::format::{hms, n, pct};
use crate::model::format::fmt_k;
use crate::model::utf16;

/// Clusters drawn as bars.
pub const CLUSTER_ROWS: usize = 10;
/// Finding cards shown before "Show N more".
pub const VISIBLE_FINDINGS: usize = peektrace_protocol::stats_text::FINDINGS_VISIBLE;
/// Hits fetched per drill page.
pub const DRILL_PAGE: f64 = 200.0;
/// Windows offered, shortest first, and the default.
pub const DAY_OPTIONS: [u32; 4] = [7, 30, 60, 90];
pub const DEFAULT_DAYS: u32 = 60;
pub const NO_SHELL: &str = "—";

/// One ranked bar row.
#[derive(Debug, Clone, PartialEq)]
pub struct RankRow {
    pub key: String,
    pub label: String,
    pub value: f64,
    pub value_text: String,
    pub secondary: String,
}

/// A bar's width in percent of the track; a non-zero row stays visible.
pub fn width_of(value: f64, max: f64) -> f64 {
    if max <= 0.0 {
        0.0
    } else {
        (value / max * 100.0).max(0.5)
    }
}

pub fn shape_rows(w: &WaitPanelView) -> Vec<RankRow> {
    w.by_shape
        .iter()
        .map(|s| RankRow {
            key: s.key.clone(),
            label: shape_label(&s.key),
            value: s.total_sec,
            value_text: hms(s.total_sec),
            secondary: format!("{} calls · {}", n(s.calls), pct(s.share)),
        })
        .collect()
}

pub fn cluster_rows(rows: &[ClusterRowView]) -> Vec<RankRow> {
    rows.iter()
        .take(CLUSTER_ROWS)
        .map(|r| RankRow {
            key: r.key.clone(),
            label: r.label.clone(),
            value: r.rows,
            value_text: format!("{} rows", n(r.rows)),
            secondary: format!("{} sessions · {}", n(r.sessions), r.tool),
        })
        .collect()
}

pub fn cluster_max(rows: &[ClusterRowView]) -> f64 {
    rows.first().map_or(1.0, |r| r.rows)
}

pub fn more_clusters(rows: &[ClusterRowView]) -> Option<String> {
    #[expect(clippy::cast_precision_loss, reason = "row counts")]
    (rows.len() > CLUSTER_ROWS).then(|| {
        format!(
            "{} more clusters not shown.",
            n((rows.len() - CLUSTER_ROWS) as f64)
        )
    })
}

/// The waiting figures: (value, label).
pub fn wait_figures(w: &WaitPanelView) -> [(String, &'static str); 6] {
    [
        (hms(w.total_sec), "waiting"),
        (pct(w.share), "of bash time"),
        (n(w.calls), "calls"),
        (n(w.sessions), "sessions"),
        (hms(w.median_sec), "median"),
        (hms(w.p95_sec), "p95"),
    ]
}

pub fn wait_note(w: &WaitPanelView) -> Option<String> {
    (w.bg_runs > 0.0).then(|| {
        format!(
            "{} backgrounded waits are charged {} while declaring {} of literal sleep — the declared seconds are printed here and never summed into a total.",
            n(w.bg_runs),
            hms(w.bg_sec),
            hms(w.bg_declared_sec)
        )
    })
}

pub fn corpus_line(c: &CorpusView) -> String {
    let (main, nested) = c.split();
    format!(
        "{} sessions + {} subagent runs · {} projects · {} tool calls · {} shell calls · {} of shell time",
        n(main),
        n(nested),
        n(c.projects),
        n(c.tool_calls),
        n(c.bash_calls),
        hms(c.bash_sec)
    )
}

pub fn window_line(r: &StatsReportView, generated: &str) -> String {
    format!(
        "{} → {} · generated {generated} · {} · detector v{}",
        utf16::slice(&r.window.from_iso, 0, 10),
        utf16::slice(&r.window.to_iso, 0, 10),
        r.schema_version,
        peektrace_js::num::to_js_string(r.detector_version)
    )
}

#[expect(clippy::cast_precision_loss, reason = "list lengths")]
pub fn caveats_line(r: &StatsReportView) -> String {
    let skipped = if r.skipped.is_empty() {
        String::new()
    } else {
        format!(" · {} files skipped", n(r.skipped.len() as f64))
    };
    format!(
        "{}{skipped}",
        peektrace_protocol::stats_text::measured_link(r.caveats.len())
    )
}

/// The cause's id badge: `row.key.replace("cause:", "")`, first match only.
pub fn cause_badge(key: &str) -> String {
    key.replacen("cause:", "", 1)
}

/// The cause table's cells, wide to narrow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CauseCells {
    pub hits: String,
    pub retries: String,
    pub retry_sessions: String,
    pub hits_split: String,
    pub retry_time: String,
    pub ctx: String,
}

/// `17.4M · ~$9`: re-read tokens and their approximate price.
pub fn reread_cell(tokens: f64, usd: Option<f64>) -> String {
    usd.map_or_else(
        || fmt_k(tokens),
        |u| format!("{} · {}", fmt_k(tokens), approx_usd_text(u, n)),
    )
}

/// A waiting shape in plain words; an unknown key prints as sent.
fn shape_label(key: &str) -> String {
    use peektrace_protocol::wire::stats::WaitShape as W;
    let shape = match key {
        "loop-poll" => W::LoopPoll,
        "watcher" => W::Watcher,
        "curl-retry" => W::CurlRetry,
        "bare-delay" => W::BareDelay,
        _ => return key.to_owned(),
    };
    wait_shape_label(shape).to_owned()
}

pub fn cause_cells(r: &CauseRowView) -> CauseCells {
    CauseCells {
        hits: n(r.hits),
        retries: n(r.pairs),
        retry_sessions: format!("{} sessions", n(r.retry_sessions)),
        hits_split: format!("{} / {}", n(r.hits_by.main), n(r.hits_by.nested)),
        retry_time: if r.sec_measured == 0.0 {
            NO_SHELL.to_owned()
        } else {
            format!("{} · {}", hms(r.retry_sec), pct(r.retry_share))
        },
        ctx: if r.tokens_measured == 0.0 {
            NO_SHELL.to_owned()
        } else {
            reread_cell(r.retry_ctx_tokens, r.retry_usd)
        },
    }
}

pub fn cause_learned(r: &CauseRowView) -> Option<String> {
    (!r.diffs.is_empty()).then(|| {
        let items: Vec<String> = r
            .diffs
            .iter()
            .map(|d| format!("{} → {} ({})", d.from, d.to, n(d.pairs)))
            .collect();
        format!("learned: {}", items.join(" · "))
    })
}

pub fn cause_named(r: &CauseRowView) -> Option<String> {
    (!r.details.is_empty()).then(|| {
        let items: Vec<String> = r
            .details
            .iter()
            .map(|d| format!("{} {}", d.value, n(d.hits)))
            .collect();
        format!("named: {}", items.join(" · "))
    })
}

pub fn cause_pairs(r: &CauseRowView) -> Vec<String> {
    r.pair_samples
        .iter()
        .map(|p| format!("{} → {}", p.failed.line, p.retry.line))
        .collect()
}

/// A rule match line: `{path}:{line} {excerpt}`.
pub fn match_lines(r: &CauseRowView) -> Vec<String> {
    r.rule.as_ref().map_or_else(Vec::new, |rule| {
        rule.matches
            .iter()
            .map(|m| {
                format!(
                    "{}:{} {}",
                    m.path,
                    peektrace_js::num::to_js_string(m.line),
                    m.excerpt
                )
            })
            .collect()
    })
}

/// The time row's badges: capped and backgrounded runs.
pub fn time_badges(r: &TimeRowView) -> Vec<String> {
    let mut out = Vec::new();
    if r.capped_runs > 0.0 {
        out.push(format!("{} capped", n(r.capped_runs)));
    }
    if r.bg_runs > 0.0 {
        out.push(format!("{} bg", n(r.bg_runs)));
    }
    out
}

/// Time, share, runs, in other jobs, median, p95.
pub fn time_cells(r: &TimeRowView) -> [String; 6] {
    [
        hms(r.total_sec),
        pct(r.share),
        n(r.runs),
        n(r.co_present_runs),
        hms(r.median_sec),
        hms(r.p95_sec),
    ]
}

pub fn co_present_note(r: &TimeRowView) -> String {
    format!(
        "{} lines also run this job but are charged to another one",
        n(r.co_present_runs)
    )
}

pub fn backgrounded(r: &TimeRowView) -> Option<String> {
    (r.bg_runs > 0.0).then(|| {
        format!(
            "backgrounded: {} runs, {} charged",
            n(r.bg_runs),
            hms(r.bg_sec)
        )
    })
}

pub fn show_more(total: usize) -> Option<String> {
    (total > VISIBLE_FINDINGS).then(|| format!("Show {} more", total - VISIBLE_FINDINGS))
}

/// One drill page's position, and the offsets its buttons go to.
#[derive(Debug, Clone, PartialEq)]
pub struct PageNote {
    pub note: String,
    pub more: Option<(String, f64)>,
    pub back: Option<f64>,
}

#[expect(clippy::cast_precision_loss, reason = "page lengths")]
pub fn page_note(offset: f64, shown: usize, total: f64) -> PageNote {
    let next = offset + shown as f64;
    let left = total - next;
    PageNote {
        note: format!("{}–{} of {} calls", n(offset + 1.0), n(next), n(total)),
        more: (left > 0.0).then(|| (format!("Load {} more", n(left.min(DRILL_PAGE))), next)),
        back: (offset > 0.0).then(|| (offset - DRILL_PAGE).max(0.0)),
    }
}

/// `daysOf`: a window the select offers, else the default.
pub fn days_of(raw: Option<&str>) -> u32 {
    let parsed = raw.map_or(0.0, peektrace_js::num::parse_number);
    DAY_OPTIONS
        .into_iter()
        .find(|d| f64::from(*d).total_cmp(&parsed).is_eq())
        .unwrap_or(DEFAULT_DAYS)
}

/// `agentOf`: a known agent id, else every agent.
pub fn agent_of(raw: Option<&str>) -> Option<AgentId> {
    raw.and_then(AgentId::parse)
}

/// `parseDrill`: `table:key`, split at the first colon.
pub fn parse_drill(raw: Option<&str>) -> Option<(DrillTable, String)> {
    let (table, key) = raw?.split_once(':')?;
    let table = match table {
        "cause" => DrillTable::Cause,
        "fail" => DrillTable::Fail,
        "time" => DrillTable::Time,
        "cluster" => DrillTable::Cluster,
        "wait" => DrillTable::Wait,
        _ => return None,
    };
    (!key.is_empty()).then(|| (table, key.to_owned()))
}

/// The detector a drill's rows are marked with: the cause id, or the fail
/// row's detector.
pub fn detector_of(r: &StatsReportView, table: DrillTable, key: &str) -> Option<String> {
    match table {
        DrillTable::Cause => r
            .causes
            .iter()
            .find(|c| c.key == key)
            .map(|c| c.cause.clone()),
        DrillTable::Fail => r
            .fails
            .iter()
            .find(|f| f.key == key)
            .and_then(|f| f.detector.clone()),
        _ => None,
    }
}

pub const fn table_name(t: DrillTable) -> &'static str {
    match t {
        DrillTable::Cause => "cause",
        DrillTable::Fail => "fail",
        DrillTable::Time => "time",
        DrillTable::Cluster => "cluster",
        DrillTable::Wait => "wait",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_and_pages() {
        assert!((width_of(5.0, 10.0) - 50.0).abs() < 1e-9);
        assert!((width_of(0.0, 10.0) - 0.5).abs() < 1e-9);
        assert!(width_of(3.0, 0.0).abs() < 1e-9);
        let p = page_note(0.0, 200, 450.0);
        assert_eq!(p.note, "1–200 of 450 calls");
        assert_eq!(p.more, Some(("Load 200 more".to_owned(), 200.0)));
        assert_eq!(p.back, None);
        let last = page_note(400.0, 50, 450.0);
        assert_eq!(last.more, None);
        assert_eq!(last.back, Some(200.0));
        assert_eq!(page_note(0.0, 0, 0.0).note, "1–0 of 0 calls");
    }

    #[test]
    fn hash_params() {
        assert_eq!(days_of(Some("30")), 30);
        assert_eq!(days_of(Some("31")), 60);
        assert_eq!(days_of(None), 60);
        assert_eq!(agent_of(Some("pi")), Some(AgentId::Pi));
        assert_eq!(agent_of(Some("all")), None);
        assert_eq!(
            parse_drill(Some("cause:cause:zsh")),
            Some((DrillTable::Cause, "cause:zsh".to_owned()))
        );
        assert_eq!(parse_drill(Some("cause:")), None);
        assert_eq!(parse_drill(Some("nope:x")), None);
        assert_eq!(cause_badge("cause:a:cause:b"), "a:cause:b");
    }
}
