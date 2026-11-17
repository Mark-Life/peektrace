//! The line a stats page opens with: what avoidable retries cost in this
//! window, in time, share and tokens, and how much of it the shown fixes
//! cover. Each share divides by the time its own causes ran in.

pub use peektrace_protocol::stats_text::FINDINGS_VISIBLE;

use super::accumulate::DrillTable;
use super::cause_text::{counted, reread_text, time_share};
use super::detectors::cause_row::CauseTotals;
use super::findings::{Finding, FindingFormat};
use super::report::StatsReport;

/// `Avoidable retries cost 1h54m (0.8% of shell time) and 55.5M tokens
/// re-read (~$83) in 60 days. The 3 fixes below cover 56.8% of retries.`
/// `shown` is how many findings the surface lists before "more".
pub fn verdict_with(r: &StatsReport, found: &[Finding], shown: usize, f: &FindingFormat) -> String {
    let t = CauseTotals::of(&r.causes, &r.corpus);
    let days = counted(r.window.days, "day", "days", f);
    if t.pairs == 0.0 {
        return format!("No avoidable retries in {days}.");
    }
    let cost = format!(
        "Avoidable retries cost {} and {} in {days}.",
        time_share(t.retry_sec, t.retry_share, t.shell, f),
        reread_text(t.retry_ctx_tokens, t.retry_usd, f)
    );
    let fixes: Vec<&str> = found
        .iter()
        .take(shown)
        .filter(|x| x.table == Some(DrillTable::Cause))
        .map(|x| x.key.as_str())
        .collect();
    let covered: f64 = r
        .causes
        .iter()
        .filter(|c| fixes.contains(&c.key.as_str()))
        .map(|c| c.pairs)
        .sum();
    let cover = match fixes.len() {
        0 => String::new(),
        1 => format!(
            " The fix below covers {} of retries.",
            (f.pct)(covered / t.pairs)
        ),
        n => format!(
            " The {n} fixes below cover {} of retries.",
            (f.pct)(covered / t.pairs)
        ),
    };
    format!("{cost}{cover}")
}

/// What the verdict's total holds that no fix removes, or `None`.
pub fn verdict_note_with(r: &StatsReport, f: &FindingFormat) -> Option<String> {
    let t = CauseTotals::of(&r.causes, &r.corpus);
    let read_backs = (t.non_failing_pairs > 0.0).then(|| {
        format!(
            "{} of large output, which {} not failures,",
            counted(t.non_failing_pairs, "read-back", "read-backs", f),
            if t.non_failing_pairs == 1.0 {
                "is"
            } else {
                "are"
            }
        )
    });
    let unnamed = (t.unclassified_pairs > 0.0).then(|| {
        format!(
            "{} with no named cause, which {} no fix",
            counted(t.unclassified_pairs, "retry", "retries", f),
            if t.unclassified_pairs == 1.0 {
                "has"
            } else {
                "have"
            }
        )
    });
    let parts: Vec<String> = read_backs.into_iter().chain(unnamed).collect();
    let parts = parts.join(" and ");
    let parts = parts.strip_suffix(',').unwrap_or(&parts);
    (!parts.is_empty()).then(|| format!("The total includes {parts}."))
}
