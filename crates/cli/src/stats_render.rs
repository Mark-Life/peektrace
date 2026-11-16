//! Text renderers for `peektrace stats`: the report, a drill page, the
//! usage view and the refresh summary.

use peektrace_core::js::num::{to_fixed, to_js_string};
use peektrace_core::js::units;
use peektrace_core::stats::accumulate::SampleRef;
use peektrace_core::stats::detectors::failures::FailRow;
use peektrace_core::stats::detectors::pairing::RETRY_WINDOW;
use peektrace_core::stats::detectors::retries::CauseRow;
use peektrace_core::stats::detectors::timing::TimeRow;
use peektrace_core::stats::detectors::usage::{UsageBy, UsageTotals};
use peektrace_core::stats::findings::{
    NO_RULE, findings_of, no_suggestion_reason, rule_line, suggested_rules,
};
use peektrace_core::stats::format::{hms, percent, rate_text, tokens};
use peektrace_core::stats::schema::{DETECTOR_VERSION, Job, STATS_SCHEMA_VERSION};
use peektrace_core::stats::service::{DrillResult, StatsReport};
use peektrace_core::stats::usage_service::{UsageReport, zone_of};

use crate::render::table;

const CELL_MAX: usize = 72;
const REFS_PER_ROW: usize = 3;
const FINDINGS_SHOWN: usize = 5;

fn job_fix(job: Job) -> &'static str {
    match job {
        Job::WaitPoll => {
            "poll a condition instead of sleeping; background the job and check it once"
        }
        Job::Test => "scope the run to the changed package or file",
        Job::Build => "reuse the build cache; build once per change, not once per check",
        Job::Typecheck => "typecheck the package you touched, not the repo",
        Job::Lint => "lint the changed files",
        Job::PackageInstall => "install from the lockfile once per environment",
        Job::Net => "cache the response; a repeat fetch inside one run pays twice",
        Job::ScriptRun => "name a recurring one-off as a script so it is cached and reviewed",
        Job::Git => "batch status/diff into one call",
        Job::Search => "narrow the path; search the package, not the repo",
        Job::FileIo | Job::Other => NO_RULE,
    }
}

fn short_sid(sid: &str) -> String {
    let head = sid.split('-').next().unwrap_or(sid);
    if units::len(head) >= 8 {
        head.to_owned()
    } else {
        units::slice(sid, 0, 14)
    }
}

fn clip_cell(s: &str) -> String {
    if units::len(s) > CELL_MAX {
        format!("{}…", units::slice(s, 0, CELL_MAX - 1))
    } else {
        s.to_owned()
    }
}

fn hit_ref(parent: Option<&str>, sid: &str) -> String {
    match parent {
        None => short_sid(sid),
        Some(p) => format!("{}/{}", short_sid(p), short_sid(sid)),
    }
}

fn refs_of(samples: &[SampleRef]) -> String {
    let mut ids: Vec<String> = Vec::new();
    for s in samples {
        let id = match &s.parent {
            None => short_sid(&s.sid),
            Some(p) => format!("{}/sub", short_sid(p)),
        };
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids.truncate(REFS_PER_ROW);
    if ids.is_empty() {
        "-".to_owned()
    } else {
        ids.join(",")
    }
}

fn share_cell(fraction: f64, bash_sec: f64) -> String {
    if bash_sec > 0.0 {
        percent(fraction)
    } else {
        "not measured".to_owned()
    }
}

fn n(x: f64) -> String {
    to_js_string(x)
}

fn findings_block(r: &StatsReport) -> String {
    let found = findings_of(r);
    if found.is_empty() {
        return "FIX FIRST  no row names a change in this window".to_owned();
    }
    let mut lines = vec!["FIX FIRST  (named causes lead; never a blended score)".to_owned()];
    for (i, f) in found.iter().take(FINDINGS_SHOWN).enumerate() {
        lines.push(format!("{}. {}  [{}]", i + 1, f.title, f.impact));
        lines.push(format!("   {}", f.note));
        lines.push(format!("   {}", f.spread));
    }
    if found.len() > FINDINGS_SHOWN {
        lines.push(format!(
            "   … {} more in the tables below",
            found.len() - FINDINGS_SHOWN
        ));
    }
    lines.join("\n")
}

fn cause_cells(row: &CauseRow) -> Vec<String> {
    vec![
        row.key
            .strip_prefix("cause:")
            .unwrap_or(&row.key)
            .to_owned(),
        row.kind.as_str().to_owned(),
        n(row.hits),
        n(row.pairs),
        n(row.retry_sessions),
        n(row.projects),
        format!("{}/{}", n(row.hits_by.main), n(row.hits_by.nested)),
        format!(
            "{}/{}",
            rate_text(row.per1k.main, row.hits_by.main),
            rate_text(row.per1k.nested, row.hits_by.nested)
        ),
        if row.sec_measured == 0.0 {
            "-".to_owned()
        } else {
            hms(row.retry_sec)
        },
        if row.tokens_measured == 0.0 {
            "-".to_owned()
        } else {
            tokens(row.retry_ctx_tokens)
        },
        row.rule.as_ref().map_or_else(|| "-".to_owned(), rule_line),
        clip_cell(row.fix.as_deref().unwrap_or(NO_RULE)),
        refs_of(&row.samples),
        format!("--cause {}", row.key),
    ]
}

fn suggested_block(r: &StatsReport) -> String {
    match no_suggestion_reason(r) {
        Some(none) => format!("SUGGESTED RULES  none: {none}"),
        None => std::iter::once(
            "SUGGESTED RULES  (copy into CLAUDE.md / AGENTS.md; causes already stated are not repeated)".to_owned(),
        )
        .chain(suggested_rules(r).iter().map(|l| format!("- {l}")))
        .collect::<Vec<_>>()
        .join("\n"),
    }
}

fn fail_cells(row: &FailRow) -> Vec<String> {
    vec![
        n(row.sessions),
        n(row.rows),
        n(row.projects),
        n(row.flagged),
        row.r#where.clone(),
        clip_cell(&row.label),
        row.note
            .as_deref()
            .map_or_else(|| NO_RULE.to_owned(), clip_cell),
        refs_of(&row.samples),
        format!("--fail {}", row.key),
    ]
}

fn time_cells(row: &TimeRow, bash_sec: f64) -> Vec<String> {
    vec![
        row.label.to_owned(),
        n(row.runs),
        n(row.co_present_runs),
        hms(row.total_sec),
        share_cell(row.share, bash_sec),
        hms(row.median_sec),
        hms(row.p95_sec),
        if row.capped_runs == 0.0 {
            "-".to_owned()
        } else {
            format!("{} / {}", n(row.capped_runs), hms(row.capped_sec))
        },
        if row.bg_runs == 0.0 {
            "-".to_owned()
        } else {
            format!("{} / {}", n(row.bg_runs), hms(row.bg_sec))
        },
        job_fix(row.key).to_owned(),
        refs_of(&row.samples),
        format!("--time {}", row.key.as_str()),
    ]
}

fn wait_line(r: &StatsReport) -> String {
    let w = &r.waiting;
    if w.calls == 0.0 {
        return "WAITING  no waiting calls in this window".to_owned();
    }
    let shapes: Vec<String> = w
        .by_shape
        .iter()
        .map(|s| format!("{} {}/{}", s.key.as_str(), n(s.calls), hms(s.total_sec)))
        .collect();
    [
        format!(
            "WAITING  {} calls  {}  {} of shell time  {} sessions  {} projects",
            n(w.calls),
            hms(w.total_sec),
            share_cell(w.share, r.corpus.bash_sec),
            n(w.sessions),
            n(w.projects)
        ),
        format!("         {}", shapes.join("  ")),
        format!(
            "         backgrounded {} charged {}, declaring {} of sleep (declared is never summed in)",
            n(w.bg_runs),
            hms(w.bg_sec),
            hms(w.bg_declared_sec)
        ),
    ]
    .join("\n")
}

fn header(r: &StatsReport) -> String {
    let c = &r.corpus;
    [
        format!(
            "peektrace stats  {}d window  {} → {}",
            n(r.window.days),
            r.window.from_iso,
            r.window.to_iso
        ),
        format!(
            "{} transcripts  {} sessions  {} projects  {} tool calls  {} shell calls  {} shell time",
            n(c.transcripts),
            n(c.sessions),
            n(c.projects),
            n(c.tool_calls),
            n(c.bash_calls),
            hms(c.bash_sec)
        ),
        format!(
            "{STATS_SCHEMA_VERSION}  detector v{}  durations are approximate: they are the call → result gap",
            n(DETECTOR_VERSION)
        ),
    ]
    .join("\n")
}

fn footer(r: &StatsReport) -> String {
    let notes = r.caveats.iter().cloned().chain([
        format!("FIX '{NO_RULE}' means no rule fired for that row: drill it to read the calls"),
        format!(
            "PAIRS are failed calls followed within {} calls by a similar succeeding call; SESS counts sessions with a pair",
            n(RETRY_WINDOW)
        ),
        "HITS M/S splits hits between main sessions and subagents; PER 1K IN WINDOW M/S is hits per 1,000 calls that could have hit, per side, over the whole window (a RULE rate counts only since its rule line appeared)".to_owned(),
        "drill a row with: peektrace stats drill <DRILL>".to_owned(),
    ]);
    let skipped = (!r.skipped.is_empty()).then(|| {
        format!(
            "{} files could not be read and are not counted",
            r.skipped.len()
        )
    });
    std::iter::once("NOTES".to_owned())
        .chain(notes.chain(skipped).map(|x| format!("- {x}")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The whole text report: findings first, then the tables, then the notes.
pub fn render_report(r: &StatsReport, pretty: bool) -> String {
    let causes: Vec<Vec<String>> = r.causes.iter().map(cause_cells).collect();
    let fails: Vec<Vec<String>> = r.fails.iter().map(fail_cells).collect();
    let time: Vec<Vec<String>> = r
        .time
        .iter()
        .map(|t| time_cells(t, r.corpus.bash_sec))
        .collect();
    [
        header(r),
        String::new(),
        findings_block(r),
        String::new(),
        "AVOIDABLE RETRIES  (named causes, ranked by sessions with a retry, then sessions, then pairs)".to_owned(),
        table(
            &[
                "CAUSE", "KIND", "HITS", "PAIRS", "SESS", "PROJ", "HITS M/S", "PER 1K IN WINDOW M/S", "RETRY SEC",
                "CTX TOK", "RULE", "FIX", "SEEN IN", "DRILL",
            ],
            &causes,
            pretty,
        ),
        String::new(),
        suggested_block(r),
        String::new(),
        "WHAT FAILS  (ranked by sessions spanned, then rows)".to_owned(),
        table(
            &["SESSIONS", "ROWS", "PROJ", "FLAGGED", "WHERE", "WHAT", "FIX", "SEEN IN", "DRILL"],
            &fails,
            pretty,
        ),
        String::new(),
        "WHAT COSTS TIME  (ranked by seconds; each command is charged to exactly one job)".to_owned(),
        table(
            &[
                "JOB", "RUNS", "CO-PRESENT", "TOTAL", "SHARE", "MEDIAN", "P95", "CAPPED", "BG", "FIX", "SEEN IN", "DRILL",
            ],
            &time,
            pretty,
        ),
        String::new(),
        wait_line(r),
        String::new(),
        footer(r),
    ]
    .join("\n")
}

/// A drill page: one line naming the page, then one row per call.
pub fn render_drill(d: &DrillResult, pretty: bool) -> String {
    let paired = d.table.as_str() == "cause";
    let rows: Vec<Vec<String>> = d
        .hits
        .iter()
        .map(|h| {
            let s = &h.sample;
            let mut row = vec![
                s.ts.clone().unwrap_or_else(|| "-".to_owned()),
                hit_ref(s.parent.as_deref(), &s.sid),
                s.project.clone(),
                h.tool.clone(),
                h.dur_sec.map_or_else(|| "-".to_owned(), hms),
                if h.failed { "fail" } else { "-" }.to_owned(),
                clip_cell(&s.line),
            ];
            if paired {
                row.push(
                    h.pair_line
                        .as_deref()
                        .map_or_else(|| "-".to_owned(), clip_cell),
                );
            }
            row
        })
        .collect();
    let mut headers = vec!["TS", "SESSION", "PROJECT", "TOOL", "DUR", "STATE", "LINE"];
    if paired {
        headers.push("RETRY");
    }
    let (count, total) = (d.hits.len(), d.total);
    #[expect(clippy::cast_precision_loss, reason = "a page of hits")]
    let shown = match (d.offset, count) {
        (0.0, _) => format!("{count} of {total} calls"),
        (o, 0) => format!(
            "0 of {total} calls (offset {} is past the end)",
            to_js_string(o)
        ),
        (o, _) => format!(
            "calls {}-{} of {total}",
            to_js_string(o + 1.0),
            to_js_string(o + count as f64)
        ),
    };
    format!(
        "{} {}: {shown}\n{}",
        d.table.as_str(),
        d.key,
        table(&headers, &rows, pretty)
    )
}

fn local_time(iso: &str, tz: &str) -> String {
    let zone = zone_of(tz).unwrap_or(jiff::tz::TimeZone::UTC);
    iso.parse::<jiff::Timestamp>()
        .map(|t| t.to_zoned(zone).strftime("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

fn models_cell(models: &[String]) -> String {
    if models.is_empty() {
        return "-".to_owned();
    }
    let shown = models.iter().take(3).cloned().collect::<Vec<_>>().join(",");
    if models.len() > 3 {
        format!("{shown} +{}", models.len() - 3)
    } else {
        shown
    }
}

fn tail_cells(t: &UsageTotals) -> Vec<String> {
    vec![
        tokens(t.agg.input),
        tokens(t.agg.output),
        tokens(t.agg.cache_creation),
        tokens(t.agg.cache_read),
        tokens(t.total_tokens),
        format!("~${}", to_fixed(t.agg.usd, 2)),
        models_cell(&t.models),
    ]
}

/// The plain-text usage view: header, table, totals line, notes.
pub fn render_usage(u: &UsageReport, pretty: bool) -> String {
    let blocks = u.by == UsageBy::Block;
    let first = match u.by {
        UsageBy::Day => "DATE",
        UsageBy::Month => "MONTH",
        UsageBy::Block => "BLOCK START",
    };
    let mut headers = vec![first];
    if blocks {
        headers.extend(["END", "ACTIVE"]);
    }
    headers.extend([
        "INPUT", "OUTPUT", "CACHE W", "CACHE R", "TOTAL", "COST", "MODELS",
    ]);
    let mut rows: Vec<Vec<String>> = u
        .result
        .rows
        .iter()
        .map(|row| {
            let mut cells = match &row.block {
                None => vec![row.key.clone()],
                Some(b) => vec![
                    local_time(&b.start_iso, &u.tz),
                    units::slice(&local_time(&b.end_iso, &u.tz), 11, usize::MAX),
                    if b.active { "*" } else { "" }.to_owned(),
                ],
            };
            cells.extend(tail_cells(&row.totals));
            cells
        })
        .collect();
    if !rows.is_empty() {
        let mut totals = vec!["TOTAL".to_owned()];
        if blocks {
            totals.extend([String::new(), String::new()]);
        }
        totals.extend(tail_cells(&u.result.totals));
        rows.push(totals);
    }
    std::iter::once(format!(
        "peektrace stats usage  by {}  {}d  {}  {} model calls",
        u.by.as_str(),
        u.days,
        u.tz,
        n(u.result.totals.agg.turns)
    ))
    .chain([
        table(&headers, &rows, pretty),
        String::new(),
        "NOTES".to_owned(),
    ])
    .chain(u.result.caveats.iter().map(|c| format!("- {c}")))
    .collect::<Vec<_>>()
    .join("\n")
}
