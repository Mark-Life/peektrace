//! Every sentence a stats screen shows about one report, in the inspector's
//! number format: the web gets it over the API, the native app in process.

use std::collections::BTreeMap;

use peektrace_protocol::DrillTable as WireTable;
use peektrace_protocol::stats_text::{Finding, StatsPrompts, StatsText};
use peektrace_protocol::wire::stats::{SampleRef as WireSample, TranscriptKind};

use super::accumulate::{DrillTable, SampleRef};
use super::findings::{
    self, FindingFormat, findings_with, is_working, rule_line, suggested_rule_of, suggested_rules,
    time_note_with, working_with,
};
use super::format::count;
use super::prompts::{PromptTarget, stats_prompt_with};
use super::report::StatsReport;
use super::schema::{Kind, WriteExt};
use crate::js::inspector::{fmt_k, hms, pct};
use crate::js::json_out::Writer;
use crate::model::Agent;
use crate::wire::agent_id;

impl FindingFormat {
    /// The inspector's spelling: `39.6K` tokens, `0.42s`, `15.71%`.
    pub const INSPECTOR: Self = Self {
        count,
        hms,
        pct,
        tokens: fmt_k,
    };
}

const fn wire_table(t: DrillTable) -> WireTable {
    match t {
        DrillTable::Cause => WireTable::Cause,
        DrillTable::Fail => WireTable::Fail,
        DrillTable::Time => WireTable::Time,
        DrillTable::Cluster => WireTable::Cluster,
        DrillTable::Wait => WireTable::Wait,
    }
}

const fn wire_kind(k: Kind) -> TranscriptKind {
    match k {
        Kind::Main => TranscriptKind::Main,
        Kind::Subagent => TranscriptKind::Subagent,
        Kind::WorkflowAgent => TranscriptKind::WorkflowAgent,
    }
}

fn wire_sample(s: &SampleRef) -> WireSample {
    WireSample {
        sid: s.sid.clone(),
        agent: agent_id(s.agent),
        project: s.project.clone(),
        kind: wire_kind(s.kind),
        parent: s.parent.clone(),
        seq: s.seq,
        tool_use_id: s.tool_use_id.clone(),
        ts: s.ts.clone(),
        line: s.line.clone(),
    }
}

fn wire_finding(x: findings::Finding) -> Finding {
    Finding {
        detector: x.detector,
        impact: x.impact,
        key: x.key,
        note: x.note,
        rule: x.rule,
        spread: x.spread,
        table: x.table.map(wire_table),
        title: x.title,
        samples: x.samples.iter().map(wire_sample).collect(),
    }
}

fn prompts_of(r: &StatsReport, agent: Option<Agent>) -> StatsPrompts {
    let f = &FindingFormat::INSPECTOR;
    let ask = |t: PromptTarget| stats_prompt_with(r, agent, &t, f);
    let each = |keys: Vec<String>, make: fn(String) -> PromptTarget| {
        keys.into_iter()
            .filter_map(|k| ask(make(k.clone())).map(|p| (k, p)))
            .collect::<BTreeMap<_, _>>()
    };
    StatsPrompts {
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

/// Everything core writes for `r`, with prompts scoped to `agent`.
pub fn text_of(r: &StatsReport, agent: Option<Agent>) -> StatsText {
    let f = &FindingFormat::INSPECTOR;
    StatsText {
        findings: findings_with(r, f).into_iter().map(wire_finding).collect(),
        working: working_with(r, f).into_iter().map(wire_finding).collect(),
        suggested: suggested_rules(r).into_iter().map(str::to_owned).collect(),
        prompts: prompts_of(r, agent),
        rule_lines: r
            .causes
            .iter()
            .filter_map(|c| {
                let line = rule_line(c.rule.as_ref()?);
                let line = if is_working(c) {
                    format!("fixed · {line}")
                } else {
                    line
                };
                Some((c.key.clone(), line))
            })
            .collect(),
        suggested_rule: r
            .causes
            .iter()
            .filter_map(|c| Some((c.key.clone(), suggested_rule_of(c)?.to_owned())))
            .collect(),
        time_notes: r
            .time
            .iter()
            .filter_map(|t| Some((t.key.as_str().to_owned(), time_note_with(t, f)?)))
            .collect(),
    }
}

fn opt_text(w: &mut Writer, s: Option<&str>) {
    match s {
        Some(s) => w.text(s),
        None => w.null(),
    }
}

fn map_json(w: &mut Writer, m: &BTreeMap<String, String>) {
    w.begin_obj();
    for (k, v) in m {
        w.key_text(k).text(v);
    }
    w.end_obj();
}

fn sample_json(w: &mut Writer, s: &WireSample) {
    let kind = match s.kind {
        TranscriptKind::Main => Kind::Main,
        TranscriptKind::Subagent => Kind::Subagent,
        TranscriptKind::WorkflowAgent => Kind::WorkflowAgent,
    };
    w.elem().begin_obj();
    w.key("sid").text(&s.sid);
    w.key("agent").str(s.agent.as_str());
    w.key("project").text(&s.project);
    w.key("kind").str(kind.as_str());
    w.key("parent").opt_text(s.parent.as_deref());
    w.key("seq").num(s.seq);
    w.key("toolUseId").opt_text(s.tool_use_id.as_deref());
    w.key("ts").opt_text(s.ts.as_deref());
    w.key("line").text(&s.line);
    w.end_obj();
}

fn findings_json(w: &mut Writer, all: &[Finding]) {
    w.begin_arr();
    for x in all {
        w.elem().begin_obj();
        w.key("detector");
        opt_text(w, x.detector.as_deref());
        w.key("impact").text(&x.impact);
        w.key("key").text(&x.key);
        w.key("note").text(&x.note);
        w.key("rule");
        opt_text(w, x.rule.as_deref());
        w.key("spread").text(&x.spread);
        match x.table {
            Some(t) => w.key("table").str(t.as_str()),
            None => w.key("table").null(),
        }
        w.key("title").text(&x.title);
        w.key("samples").begin_arr();
        for sample in &x.samples {
            sample_json(w, sample);
        }
        w.end_arr();
        w.end_obj();
    }
    w.end_arr();
}

/// Write `t` as the `text` member of `GET /api/stats/report`.
pub fn write_text(w: &mut Writer, t: &StatsText) {
    w.begin_obj();
    w.key("findings");
    findings_json(w, &t.findings);
    w.key("working");
    findings_json(w, &t.working);
    w.key("suggested")
        .texts(t.suggested.iter().map(String::as_str));
    w.key("prompts").begin_obj();
    w.key("report").text(&t.prompts.report);
    w.key("cause");
    map_json(w, &t.prompts.cause);
    w.key("fail");
    map_json(w, &t.prompts.fail);
    w.key("time");
    map_json(w, &t.prompts.time);
    w.end_obj();
    w.key("ruleLines");
    map_json(w, &t.rule_lines);
    w.key("suggestedRule");
    map_json(w, &t.suggested_rule);
    w.key("timeNotes");
    map_json(w, &t.time_notes);
    w.end_obj();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_json_reads_back_as_written() {
        let mut t = StatsText::default();
        t.findings.push(Finding {
            detector: None,
            impact: "3 sessions".into(),
            key: "k\"1".into(),
            note: "note".into(),
            rule: Some("rule".into()),
            spread: "spread".into(),
            table: Some(WireTable::Fail),
            title: "title".into(),
            samples: Vec::new(),
        });
        t.working.push(Finding {
            detector: Some("zsh-glob-path".into()),
            impact: "1 retries".into(),
            key: "cause:zsh-glob-path".into(),
            note: "rule".into(),
            rule: None,
            spread: "spread".into(),
            table: None,
            title: "working".into(),
            samples: vec![WireSample {
                sid: "s1".into(),
                agent: peektrace_protocol::AgentId::Claude,
                project: "p".into(),
                kind: TranscriptKind::Main,
                parent: None,
                seq: 0.0,
                tool_use_id: None,
                ts: None,
                line: "line".into(),
            }],
        });
        t.suggested.push("line".into());
        t.prompts.report = "report".into();
        t.prompts.cause.insert("a".into(), "b".into());
        t.time_notes.insert("test".into(), "note".into());
        let mut w = Writer::new(false);
        write_text(&mut w, &t);
        let back: StatsText = serde_json::from_str(&w.finish()).expect("text json");
        assert_eq!(back, t);
    }
}
