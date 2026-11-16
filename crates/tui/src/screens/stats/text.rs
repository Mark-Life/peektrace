//! Stats text: findings in the TUI's number format, the note strip, drill
//! targets, the header lines and the caveats. Every number comes from the
//! report; nothing here derives a statistic.

use peektrace_core::Agent;
use peektrace_core::stats::accumulate::{DrillTable, SampleRef};
use peektrace_core::stats::detectors::failures::FailRow;
use peektrace_core::stats::detectors::retries::CauseRow;
use peektrace_core::stats::detectors::timing::TimeRow;
use peektrace_core::stats::findings::{
    Finding, FindingFormat, NO_RULE, cause_impact_with, cause_note, cause_spread_with,
    fail_spread_with, findings_with, time_note_with, time_spread_with,
};
use peektrace_core::stats::format::{hms, percent, short_id};
use peektrace_core::stats::report::StatsReport;
use peektrace_core::stats::schema::{DETECTOR_VERSION, Job, STATS_SCHEMA_VERSION};

use crate::fmt::{fmt, fmt_k, sanitize, width};
use crate::theme::agent_label;

/// How the TUI spells numbers in findings: `12.3K` tokens.
pub const FORMAT: FindingFormat = FindingFormat {
    count: fmt,
    hms,
    pct: percent,
    tokens: fmt_k,
};

/// Which pane owns the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    #[default]
    Findings,
    Fails,
    Time,
}

/// One row, addressed the way a drill asks for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrillTarget {
    pub key: String,
    pub label: String,
    pub note: Option<String>,
    pub spread: String,
    pub table: DrillTable,
}

pub fn findings_of(report: &StatsReport) -> Vec<Finding> {
    findings_with(report, &FORMAT)
}

pub fn short_sid(sid: &str) -> String {
    let head = short_id(sid);
    if head.chars().count() >= 8 {
        head.to_owned()
    } else {
        peektrace_core::js::units::slice(sid, 0, 14)
    }
}

pub fn ref_of(s: &SampleRef) -> String {
    s.parent
        .as_deref()
        .map_or_else(|| short_sid(&s.sid), |p| format!("{}/sub", short_sid(p)))
}

/// The command that opens a sample; a subagent opens through its parent.
pub fn open_command(sid: &str, parent: Option<&str>) -> String {
    parent.map_or_else(
        || format!("peektrace sessions analyze {sid}"),
        |p| format!("peektrace sessions analyze --parent {p} {sid}"),
    )
}

pub fn refs_of(samples: &[SampleRef]) -> String {
    let mut ids: Vec<String> = Vec::new();
    for s in samples {
        let r = ref_of(s);
        if !ids.contains(&r) {
            ids.push(r);
        }
    }
    ids.truncate(3);
    if ids.is_empty() {
        "—".to_owned()
    } else {
        ids.join(", ")
    }
}

fn seen_in(samples: &[SampleRef], tail: &str) -> String {
    format!("seen in {} · ⏎ {tail}", refs_of(samples))
}

fn wait_lines(r: &StatsReport) -> Vec<String> {
    let w = &r.waiting;
    let shapes = w
        .by_shape
        .iter()
        .map(|s| format!("{} {}/{}", s.key.as_str(), fmt(s.calls), hms(s.total_sec)))
        .collect::<Vec<_>>()
        .join(" · ");
    vec![
        if shapes.is_empty() {
            format!("{} waiting calls", fmt(w.calls))
        } else {
            shapes
        },
        format!(
            "{} backgrounded waits charged {}, declaring {} of sleep (never summed in)",
            fmt(w.bg_runs),
            hms(w.bg_sec),
            hms(w.bg_declared_sec)
        ),
    ]
}

/// What the selected row measured, how far it reaches, and the way back.
pub fn note_lines_of(
    findings: &[Finding],
    index: usize,
    pane: Pane,
    r: &StatsReport,
) -> Vec<String> {
    match pane {
        Pane::Findings => findings.get(index).map_or_else(Vec::new, |f| {
            vec![
                f.note.clone(),
                f.spread.clone(),
                seen_in(&f.samples, "reads the calls behind it"),
            ]
        }),
        Pane::Fails => r.fails.get(index).map_or_else(Vec::new, |row| {
            vec![
                row.note
                    .clone()
                    .unwrap_or_else(|| format!("{NO_RULE} — ⏎ reads its calls")),
                fail_spread_with(row, &FORMAT),
                seen_in(&row.samples, &format!("reads all {} calls", fmt(row.rows))),
            ]
        }),
        Pane::Time => r.time.get(index).map_or_else(Vec::new, |row| {
            let mut lines = vec![
                time_note_with(row, &FORMAT).unwrap_or_else(|| {
                    format!(
                        "{} runs charged {} — ⏎ reads them",
                        fmt(row.runs),
                        hms(row.total_sec)
                    )
                }),
                format!(
                    "{} runs · {} co-present · median {} · p95 {}",
                    fmt(row.runs),
                    fmt(row.co_present_runs),
                    hms(row.median_sec),
                    hms(row.p95_sec)
                ),
            ];
            if row.key == Job::WaitPoll {
                lines.extend(wait_lines(r));
            } else {
                lines.push(seen_in(
                    &row.samples,
                    &format!("reads all {} runs", fmt(row.runs)),
                ));
            }
            lines
        }),
    }
}

/// The strip padded to the height the layout gave it.
pub fn note_strip_of(
    findings: &[Finding],
    index: usize,
    pane: Pane,
    r: &StatsReport,
    max: usize,
) -> Vec<String> {
    if max == 0 {
        return Vec::new();
    }
    let mut lines = note_lines_of(findings, index, pane, r);
    lines.resize(max.max(lines.len()), String::new());
    lines.truncate(max);
    lines
}

pub fn fail_target(row: &FailRow) -> DrillTarget {
    DrillTarget {
        key: row.key.clone(),
        label: row.label.clone(),
        note: row.note.clone(),
        spread: format!(
            "{} calls · {} sessions · {} projects · {} flagged",
            fmt(row.rows),
            fmt(row.sessions),
            fmt(row.projects),
            fmt(row.flagged)
        ),
        table: DrillTable::Fail,
    }
}

pub fn time_target(row: &TimeRow, bash_sec: f64) -> DrillTarget {
    DrillTarget {
        key: row.key.as_str().to_owned(),
        label: row.label.to_owned(),
        note: time_note_with(row, &FORMAT),
        spread: format!(
            "{} runs · {} · {}",
            fmt(row.runs),
            hms(row.total_sec),
            time_spread_with(row, bash_sec, &FORMAT)
        ),
        table: DrillTable::Time,
    }
}

fn cause_target(row: &CauseRow) -> DrillTarget {
    DrillTarget {
        key: row.key.clone(),
        label: row.label.clone(),
        note: Some(cause_note(row).unwrap_or_else(|| NO_RULE.to_owned())),
        spread: format!(
            "{} · {}",
            cause_spread_with(row, &FORMAT),
            cause_impact_with(row, &FORMAT)
        ),
        table: DrillTable::Cause,
    }
}

/// A finding resolved back to the row it came from.
pub fn finding_target(f: &Finding, r: &StatsReport) -> Option<DrillTarget> {
    match f.table {
        DrillTable::Cause => r.causes.iter().find(|x| x.key == f.key).map(cause_target),
        DrillTable::Fail => r.fails.iter().find(|x| x.key == f.key).map(fail_target),
        _ => r
            .time
            .iter()
            .find(|x| x.key.as_str() == f.key)
            .map(|x| time_target(x, r.corpus.bash_sec)),
    }
}

/// How long ago the report was made.
pub fn ago_of(generated_at: f64, now: f64) -> String {
    let mins = ((now - generated_at) / 60_000.0).floor();
    if mins < 1.0 {
        return "just now".to_owned();
    }
    if mins < 60.0 {
        return format!("{mins}m ago");
    }
    let hours = (mins / 60.0).floor();
    if hours < 24.0 {
        format!("{hours}h ago")
    } else {
        format!("{}d ago", (hours / 24.0).floor())
    }
}

/// `30d · all agents · scanned 2m ago`, plus the live markers.
pub fn scope_text(
    ago: &str,
    agent: Option<Agent>,
    days: u32,
    loading: bool,
    stale: bool,
) -> String {
    let who = agent.map_or("all agents", agent_label);
    let mut parts = vec![format!("{days}d"), who.to_owned(), format!("scanned {ago}")];
    if loading {
        parts.push("↻".to_owned());
    }
    if stale {
        parts.push("● stale · R".to_owned());
    }
    parts.join(" · ")
}

pub fn corpus_line(r: &StatsReport, wide: bool) -> String {
    let c = &r.corpus;
    let mut parts = vec![
        format!("{} transcripts", fmt(c.transcripts)),
        format!("{} projects", fmt(c.projects)),
    ];
    if wide {
        parts.push(format!("{} tool calls", fmt(c.tool_calls)));
    }
    parts.push(format!("{} shell calls", fmt(c.bash_calls)));
    parts.push(format!("{} shell time", hms(c.bash_sec)));
    parts.join(" · ")
}

pub fn provenance_line(r: &StatsReport, wide: bool) -> String {
    let schema = if wide {
        STATS_SCHEMA_VERSION
    } else {
        STATS_SCHEMA_VERSION.trim_start_matches("peektrace-")
    };
    let caveats = r.caveats.len() + usize::from(!r.skipped.is_empty());
    [
        schema.to_owned(),
        format!("detector v{DETECTOR_VERSION}"),
        if wide {
            "durations ≈ the call→result gap"
        } else {
            "durations ≈ call→result gap"
        }
        .to_owned(),
        format!("{caveats} caveats (c)"),
    ]
    .join(" · ")
}

/// Every caveat plus the skipped files, wrapped for the overlay.
pub fn caveat_lines(r: &StatsReport, w: usize) -> Vec<String> {
    let mut all: Vec<String> = r.caveats.clone();
    if !r.skipped.is_empty() {
        #[expect(clippy::cast_precision_loss, reason = "file counts")]
        let n = fmt(r.skipped.len() as f64);
        all.push(format!("{n} files could not be read and are not counted:"));
        all.extend(r.skipped.iter().cloned());
    }
    all.iter()
        .flat_map(|l| wrap(&format!("- {}", sanitize(l)), w))
        .collect()
}

/// Word-wrap to `w`, hanging continuation lines by two cells.
pub fn wrap(text: &str, w: usize) -> Vec<String> {
    let cols = w.max(8);
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text
        .split(peektrace_core::js::text::is_js_whitespace)
        .filter(|w| !w.is_empty())
    {
        let next = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if width(&next) > cols && !line.is_empty() {
            out.push(std::mem::take(&mut line));
            line = format!("  {word}");
        } else {
            line = next;
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ago_and_scope() {
        assert_eq!(ago_of(0.0, 30_000.0), "just now");
        assert_eq!(ago_of(0.0, 120_000.0), "2m ago");
        assert_eq!(ago_of(0.0, 7_200_000.0), "2h ago");
        assert_eq!(ago_of(0.0, 3.0 * 86_400_000.0), "3d ago");
        assert_eq!(
            scope_text("2m ago", None, 60, true, true),
            "60d · all agents · scanned 2m ago · ↻ · ● stale · R"
        );
    }

    #[test]
    fn wrap_keeps_every_word_and_hangs_the_continuation() {
        let text = "- one two three four five six seven eight";
        let lines = wrap(text, 16);
        assert!(lines.len() > 1);
        assert!(lines[1].starts_with("  "));
        let words: Vec<&str> = lines.iter().flat_map(|l| l.split_whitespace()).collect();
        assert_eq!(words, text.split(' ').collect::<Vec<_>>());
    }

    #[test]
    fn a_subagent_routes_through_its_parent() {
        let parent = "3f2a1b4c-9d8e-4f00-8000-000000000000";
        let s = SampleRef {
            sid: "agent-aac481ccea1ab4b5a".into(),
            agent: Agent::Claude,
            project: "p".into(),
            kind: peektrace_core::stats::schema::Kind::Subagent,
            parent: Some(parent.into()),
            seq: 3.0,
            tool_use_id: None,
            ts: None,
            line: "x | tail".into(),
        };
        assert_eq!(refs_of(std::slice::from_ref(&s)), "3f2a1b4c/sub");
        assert_eq!(
            open_command(&s.sid, s.parent.as_deref()),
            format!("peektrace sessions analyze --parent {parent} agent-aac481ccea1ab4b5a")
        );
        assert_eq!(
            open_command(parent, None),
            format!("peektrace sessions analyze {parent}")
        );
    }
}
