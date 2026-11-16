//! The core-backed text of a report in the inspector's number format:
//! findings, suggested rules, every prompt, rule lines and time notes.

use std::collections::HashMap;

use peektrace_core::Agent;
use peektrace_core::js::time::LocalZone;
use peektrace_core::stats::accumulate::DrillTable as CoreTable;
use peektrace_core::stats::findings::{
    FindingFormat, findings_with, rule_line, suggested_rule_of, suggested_rules, time_note_with,
};
use peektrace_core::stats::prompts::{PromptTarget, stats_prompt_with};
use peektrace_core::stats::service::StatsReport;
use peektrace_protocol::{AgentId, DrillTable};

use super::format::{hms, n, pct};
use super::{FindingView, Prompts, StatsText};
use crate::model::format::fmt_k;

/// How the inspector spells numbers in core's sentences: `39.6K` tokens.
pub const INSPECTOR: FindingFormat = FindingFormat {
    count: n,
    hms,
    pct,
    tokens: fmt_k,
};

pub const fn core_agent(a: AgentId) -> Agent {
    match a {
        AgentId::Claude => Agent::Claude,
        AgentId::Codex => Agent::Codex,
        AgentId::Pi => Agent::Pi,
        AgentId::Opencode => Agent::Opencode,
    }
}

pub const fn table_of(t: CoreTable) -> DrillTable {
    match t {
        CoreTable::Cause => DrillTable::Cause,
        CoreTable::Fail => DrillTable::Fail,
        CoreTable::Time => DrillTable::Time,
        CoreTable::Cluster => DrillTable::Cluster,
        CoreTable::Wait => DrillTable::Wait,
    }
}

/// Milliseconds east of UTC at the instant `ms`.
#[expect(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn offset_at(zone: &dyn LocalZone, ms: f64) -> f64 {
    let ms = ms as i64;
    let first = zone.offset_at_wall_ms(ms);
    zone.offset_at_wall_ms(ms + first) as f64
}

fn prompts_of(r: &StatsReport, agent: Option<Agent>) -> Prompts {
    let ask = |t: PromptTarget| stats_prompt_with(r, agent, &t, &INSPECTOR);
    let each = |keys: Vec<String>, make: fn(String) -> PromptTarget| -> HashMap<String, String> {
        keys.into_iter()
            .filter_map(|k| ask(make(k.clone())).map(|p| (k, p)))
            .collect()
    };
    Prompts {
        report: ask(PromptTarget::Report).unwrap_or_default(),
        cause: each(
            r.causes.iter().map(|c| c.key.clone()).collect(),
            PromptTarget::Cause,
        ),
        fail: each(
            r.fails.iter().map(|f| f.key.clone()).collect(),
            PromptTarget::Fail,
        ),
        time: each(
            r.time.iter().map(|t| t.key.as_str().to_owned()).collect(),
            PromptTarget::Time,
        ),
    }
}

/// Everything core writes for `r`, scoped to `agent`.
pub fn text_of(r: &StatsReport, agent: Option<AgentId>) -> StatsText {
    let findings = findings_with(r, &INSPECTOR)
        .into_iter()
        .map(|f| FindingView {
            detector: f.detector,
            impact: f.impact,
            key: f.key,
            note: f.note,
            rule: f.rule,
            spread: f.spread,
            table: table_of(f.table),
            title: f.title,
        })
        .collect();
    StatsText {
        findings,
        suggested: suggested_rules(r).into_iter().map(str::to_owned).collect(),
        prompts: prompts_of(r, agent.map(core_agent)),
        rule_lines: r
            .causes
            .iter()
            .filter_map(|c| Some((c.key.clone(), rule_line(c.rule.as_ref()?))))
            .collect(),
        suggested_rule: r
            .causes
            .iter()
            .filter_map(|c| Some((c.key.clone(), suggested_rule_of(c)?.to_owned())))
            .collect(),
        time_notes: r
            .time
            .iter()
            .filter_map(|t| Some((t.key.as_str().to_owned(), time_note_with(t, &INSPECTOR)?)))
            .collect(),
    }
}
