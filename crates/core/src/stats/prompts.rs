//! Investigation prompts: the report, or one row of it, as text an agent acts on.

use std::collections::HashSet;

use super::accumulate::SampleRef;
use super::detectors::causes::CauseKind;
use super::detectors::failures::FailRow;
use super::detectors::retries::{CauseRow, PairSample};
use super::detectors::timing::TimeRow;
use super::findings::{
    RuleState, cause_impact, cause_spread, rule_line, rule_state, time_note, time_spread,
};
use super::format::{count, hms, percent, tokens};
use super::rank::DEFAULT_DRILL_PAGE;
use super::service::StatsReport;
use crate::js::num::to_js_string;
use crate::js::regex::LazyRe;
use crate::js::units;
use crate::js_re;
use crate::model::Agent;
use crate::stats::schema::Job;

/// What a prompt is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptTarget {
    Report,
    Cause(String),
    Fail(String),
    Time(String),
}

const TOP_CAUSES: usize = 6;
const TOP_FAILS: usize = 4;
const TOP_TIME: usize = 4;
const SAMPLES_SHOWN: usize = 3;
const GOAL: &str = "Goal: make my coding agents faster and cheaper: fewer model round trips, less compute and waiting, fewer failed tool calls.";
const ASK: &str = "Propose concrete changes to my environment (shell config, installed tools, hooks), AGENTS.md/CLAUDE.md, skills or prompts that prevent it. Rank by impact; show exact edits; don't apply until I confirm.";

static MS_SUFFIX: LazyRe = js_re!(r"\.\d+Z$");
static KEEPS_FIRING: LazyRe = js_re!(r"^if it keeps firing:\s*");

const fn kind_where(k: CauseKind) -> &'static str {
    match k {
        CauseKind::Rule => "memory file (AGENTS.md/CLAUDE.md)",
        CauseKind::Env => "host environment",
        CauseKind::Workflow => "workflow or subagent prompt",
        CauseKind::None => "unknown (unclassified)",
    }
}

struct Ctx<'a> {
    report: &'a StatsReport,
    agent: Option<Agent>,
}

impl Ctx<'_> {
    fn command(&self, sub: &str) -> String {
        let mut parts = vec!["peektrace".to_owned(), "stats".to_owned()];
        if !sub.is_empty() {
            parts.push(sub.to_owned());
        }
        parts.push(format!("--days {}", to_js_string(self.report.window.days)));
        if let Some(a) = self.agent {
            parts.push(format!("--agent {}", a.as_str()));
        }
        parts.join(" ")
    }

    fn drill_line(&self, flag: &str, key: &str) -> String {
        format!(
            "Calls (first {DEFAULT_DRILL_PAGE}, .total counts all, .paths has transcripts): {} --{flag} {key} --json",
            self.command("drill")
        )
    }

    fn path_of(&self, sid: &str) -> Option<&str> {
        self.report
            .paths
            .iter()
            .find(|(s, _)| s == sid)
            .map(|(_, p)| p.as_str())
    }

    fn sample_lines(&self, sample: &SampleRef, retry: Option<&SampleRef>) -> [String; 2] {
        let head: Vec<String> = [
            sample
                .ts
                .as_deref()
                .map(|t| MS_SUFFIX.replace_first(t, |_| "Z".to_owned())),
            sample.tool_use_id.clone(),
            Some(
                self.path_of(&sample.sid)
                    .map_or_else(|| format!("session {}", sample.sid), str::to_owned),
            ),
        ]
        .into_iter()
        .flatten()
        .collect();
        [
            format!("- {}", head.join(" · ")),
            match retry {
                None => format!("  call: {}", sample.line),
                Some(r) => format!("  failed: {}\n  retry: {}", sample.line, r.line),
            },
        ]
    }

    fn samples_block(&self, pairs: &[PairSample], samples: &[SampleRef]) -> Vec<String> {
        let id_of = |s: &SampleRef| format!("{}#{}", s.sid, to_js_string(s.seq));
        let paired: HashSet<String> = pairs.iter().map(|p| id_of(&p.failed)).collect();
        let lines: Vec<String> = pairs
            .iter()
            .map(|p| (&p.failed, Some(&p.retry)))
            .chain(
                samples
                    .iter()
                    .filter(|s| !paired.contains(&id_of(s)))
                    .map(|s| (s, None)),
            )
            .take(SAMPLES_SHOWN)
            .flat_map(|(s, r)| self.sample_lines(s, r))
            .collect();
        if lines.is_empty() {
            return lines;
        }
        let head = if pairs.is_empty() {
            "Samples:"
        } else {
            "Samples (failed → retry):"
        };
        std::iter::once(head.to_owned()).chain(lines).collect()
    }
}

fn cause_id(row: &CauseRow) -> &str {
    row.key.strip_prefix("cause:").unwrap_or(&row.key)
}

fn capitalized(text: &str) -> String {
    let mut c = text.chars();
    c.next()
        .map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

fn rule_ask(row: &CauseRow) -> Vec<String> {
    let Some(rule) = &row.rule else {
        return if row.kind == CauseKind::Rule {
            vec!["- rule: none in AGENTS.md/CLAUDE.md".to_owned()]
        } else {
            vec![]
        };
    };
    let line = format!("- {}", rule_line(rule));
    match rule_state(rule) {
        RuleState::Firing => vec![
            line,
            format!(
                "  The rule is in memory yet still fires: find why it isn't followed and fix it in a way that doesn't depend on the agent remembering (e.g. {}).",
                KEEPS_FIRING.replace_first(&rule.advice, |_| String::new())
            ),
        ],
        RuleState::Unmeasured => vec![line, format!("  {}.", capitalized(&rule.advice))],
        RuleState::Quiet => vec![line],
    }
}

fn cause_prompt(c: &Ctx<'_>, row: &CauseRow) -> Vec<String> {
    let mut out = vec![format!("Pattern: {}", row.label)];
    let where_ = if row.rule.is_none() {
        format!(" · fix lives in: {}", kind_where(row.kind))
    } else {
        String::new()
    };
    out.push(format!("- cause: {}{where_}", cause_id(row)));
    out.push(format!(
        "- {} sessions · {}",
        count(row.sessions),
        cause_spread(row)
    ));
    if row.pairs > 0.0 {
        out.push(format!("- wasted: {}", cause_impact(row)));
    }
    if !row.details.is_empty() {
        let items: Vec<String> = row
            .details
            .iter()
            .map(|(v, n)| format!("{v} ×{}", count(*n)))
            .collect();
        out.push(format!("- culprits: {}", items.join(" · ")));
    }
    if !row.diffs.is_empty() {
        let items: Vec<String> = row
            .diffs
            .iter()
            .map(|(f, t, n)| format!("{f} → {t} ×{}", count(*n)))
            .collect();
        out.push(format!("- retries changed: {}", items.join(" · ")));
    }
    if let Some(fix) = &row.fix {
        out.push(format!("- suggested fix: {fix}"));
    }
    out.extend(rule_ask(row));
    out.extend(c.samples_block(&row.pair_samples, &row.samples));
    out.push(c.drill_line("cause", cause_id(row)));
    out
}

fn fail_prompt(c: &Ctx<'_>, row: &FailRow) -> Vec<String> {
    let det = row
        .detector
        .as_ref()
        .map_or_else(String::new, |d| format!(" · detector: {d}"));
    let mut out = vec![
        format!("Pattern: {} ({})", row.label, row.r#where),
        format!("- key: {}{det}", row.key),
        format!(
            "- calls: {} ({} set the error flag) in {} sessions, {} projects",
            count(row.rows),
            count(row.flagged),
            count(row.sessions),
            count(row.projects)
        ),
    ];
    if let Some(n) = &row.note {
        out.push(format!("- note: {n}"));
    }
    out.extend(c.samples_block(&[], &row.samples));
    out.push(c.drill_line("fail", &row.key));
    out
}

fn wait_lines(r: &StatsReport) -> Vec<String> {
    let w = &r.waiting;
    if w.calls == 0.0 {
        return vec![];
    }
    let shapes: Vec<String> = w
        .by_shape
        .iter()
        .filter(|s| s.calls > 0.0)
        .map(|s| {
            format!(
                "{} {} calls {}",
                s.key.as_str(),
                count(s.calls),
                hms(s.total_sec)
            )
        })
        .collect();
    vec![format!(
        "- shapes: {}; {} backgrounded, declaring {} of sleep",
        shapes.join(" · "),
        count(w.bg_runs),
        hms(w.bg_declared_sec)
    )]
}

fn time_prompt(c: &Ctx<'_>, row: &TimeRow) -> Vec<String> {
    let mut out = vec![
        format!("Pattern: shell time charged to {}", row.label),
        format!(
            "- job: {} · {} · {} · median {}, p95 {}",
            row.key.as_str(),
            hms(row.total_sec),
            time_spread(row, c.report.corpus.bash_sec),
            hms(row.median_sec),
            hms(row.p95_sec)
        ),
    ];
    if row.key == Job::WaitPoll {
        out.extend(wait_lines(c.report));
    }
    if let Some(n) = time_note(row) {
        out.push(format!("- note: {n}"));
    }
    out.extend(c.samples_block(&[], &row.samples));
    out.push(c.drill_line("time", row.key.as_str()));
    out
}

fn section(header: &str, rows: Vec<String>) -> Vec<String> {
    if rows.is_empty() {
        rows
    } else {
        std::iter::once(header.to_owned()).chain(rows).collect()
    }
}

fn rule_lines(causes: &[&CauseRow]) -> Vec<String> {
    let ids = |state: RuleState| -> Vec<String> {
        causes
            .iter()
            .filter_map(|r| {
                let rule = r.rule.as_ref().filter(|x| rule_state(x) == state)?;
                Some(if state == RuleState::Firing {
                    format!("{} ({})", cause_id(r), rule_line(rule))
                } else {
                    cause_id(r).to_owned()
                })
            })
            .collect()
    };
    let (firing, quiet, unmeasured) = (
        ids(RuleState::Firing),
        ids(RuleState::Quiet),
        ids(RuleState::Unmeasured),
    );
    let mut out = Vec::new();
    if !firing.is_empty() || !quiet.is_empty() {
        let f = if firing.is_empty() {
            "none".to_owned()
        } else {
            firing.join("; ")
        };
        out.push(format!("- rules in memory but still firing: {f}"));
    }
    if !unmeasured.is_empty() {
        out.push(format!(
            "- rules in memory, still-firing rate not measured yet: {}",
            unmeasured.join(", ")
        ));
    }
    out
}

/// The causes and failures sections of the report prompt.
fn table_sections(r: &StatsReport, causes: &[&CauseRow]) -> Vec<String> {
    let mut out = Vec::new();
    out.extend(section(
        "- causes (--cause id):",
        causes
            .iter()
            .take(TOP_CAUSES)
            .map(|x| {
                let rule = if x.rule.is_none() {
                    ""
                } else {
                    " · rule in memory"
                };
                format!(
                    "  - {}: {} hits · {}{rule}",
                    cause_id(x),
                    count(x.hits),
                    cause_impact(x)
                )
            })
            .collect(),
    ));
    out.extend(section(
        "- failures (--fail key):",
        r.fails
            .iter()
            .take(TOP_FAILS)
            .map(|x| {
                let note = x
                    .note
                    .as_ref()
                    .map_or_else(String::new, |n| format!(" · {n}"));
                format!(
                    "  - {} ({}): {} calls, {} sessions{note}",
                    x.key,
                    x.label,
                    count(x.rows),
                    count(x.sessions)
                )
            })
            .collect(),
    ));
    out
}

fn report_body(c: &Ctx<'_>) -> Vec<String> {
    let r = c.report;
    let corp = &r.corpus;
    let main = corp
        .kinds
        .iter()
        .find(|(k, _)| k == "main")
        .map_or(0.0, |(_, n)| *n);
    let causes: Vec<&CauseRow> = r.causes.iter().filter(|x| x.hits > 0.0).collect();
    let pairs = causes.iter().fold(0.0, |n, x| n + x.pairs);
    let sec = causes.iter().fold(0.0, |n, x| n + x.retry_sec);
    let ctx = causes.iter().fold(0.0, |n, x| n + x.retry_ctx_tokens);
    let time: Vec<String> = r
        .time
        .iter()
        .take(TOP_TIME)
        .map(|t| {
            format!(
                "{} {} ({})",
                t.key.as_str(),
                hms(t.total_sec),
                percent(t.share)
            )
        })
        .collect();
    let mut out = vec![
        format!(
            "Data ({} → {}):",
            units::slice(&r.window.from_iso, 0, 10),
            units::slice(&r.window.to_iso, 0, 10)
        ),
        format!(
            "- corpus: {} transcripts ({} main, {} subagent), {} projects, {} tool calls, {} shell time",
            count(corp.transcripts),
            count(main),
            count(corp.transcripts - main),
            count(corp.projects),
            count(corp.tool_calls),
            hms(corp.bash_sec)
        ),
    ];
    if pairs > 0.0 {
        out.push(format!(
            "- avoidable retries: {} round trips, {}, {} ctx tokens re-read",
            count(pairs),
            hms(sec),
            tokens(ctx)
        ));
    }
    out.extend(table_sections(r, &causes));
    if !time.is_empty() {
        out.push(format!("- shell time (--time job): {}", time.join(" · ")));
    }
    if r.waiting.calls > 0.0 {
        out.push(format!(
            "- waiting: {} calls, {}, {} backgrounded",
            count(r.waiting.calls),
            hms(r.waiting.total_sec),
            count(r.waiting.bg_runs)
        ));
    }
    out.extend(rule_lines(&causes));
    out.push("More data (--cause, --fail or --time picks the row):".to_owned());
    out.push(format!("- full report: {} --json", c.command("")));
    out.push(format!(
        "- one row, with transcript paths: {} --cause <id>",
        c.command("prompt")
    ));
    out.push(format!(
        "- a row's calls: {} --cause <id> --json",
        c.command("drill")
    ));
    out
}

/// The prompt text, or `None` when the target names no row.
pub fn stats_prompt(
    report: &StatsReport,
    agent: Option<Agent>,
    target: &PromptTarget,
) -> Option<String> {
    let c = Ctx { report, agent };
    let where_ = format!(
        "last {} days, {}",
        to_js_string(report.window.days),
        agent.map_or("all agents", Agent::as_str)
    );
    let (lead, body) = if *target == PromptTarget::Report {
        (
            format!(
                "Below is measured data from peektrace ({where_}). Pick the patterns worth fixing, pull their calls with the commands at the end, and read the transcripts to find root causes. {ASK}"
            ),
            report_body(&c),
        )
    } else {
        {
            let body = match target {
                PromptTarget::Cause(k) => report
                    .causes
                    .iter()
                    .find(|r| r.key == *k || cause_id(r) == k)
                    .map(|r| cause_prompt(&c, r)),
                PromptTarget::Fail(k) => report
                    .fails
                    .iter()
                    .find(|r| r.key == *k)
                    .map(|r| fail_prompt(&c, r)),
                PromptTarget::Time(k) => report
                    .time
                    .iter()
                    .find(|r| r.key.as_str() == k)
                    .map(|r| time_prompt(&c, r)),
                PromptTarget::Report => None,
            }?;
            (
                format!(
                    "Below is measured data for one pattern from peektrace ({where_}). Read the sample transcripts to confirm it and find its root cause. {ASK}"
                ),
                body,
            )
        }
    };
    Some(
        [GOAL.to_owned(), lead, String::new()]
            .into_iter()
            .chain(body)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}
