//! Every sentence a stats screen shows about one report, in the inspector's
//! number format: the web gets it over the API, the native app in process.

use std::collections::BTreeMap;

use peektrace_protocol::DrillTable as WireTable;
use peektrace_protocol::stats_text::{Finding, StatsPrompts, StatsText};

use super::accumulate::DrillTable;
use super::findings::{
    FindingFormat, findings_with, rule_line, suggested_rule_of, suggested_rules, time_note_with,
};
use super::format::count;
use super::prompts::{PromptTarget, stats_prompt_with};
use super::report::StatsReport;
use crate::js::inspector::{fmt_k, hms, pct};
use crate::js::json_out::Writer;
use crate::model::Agent;

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
    let findings = findings_with(r, f)
        .into_iter()
        .map(|x| Finding {
            detector: x.detector,
            impact: x.impact,
            key: x.key,
            note: x.note,
            rule: x.rule,
            spread: x.spread,
            table: wire_table(x.table),
            title: x.title,
        })
        .collect();
    StatsText {
        findings,
        suggested: suggested_rules(r).into_iter().map(str::to_owned).collect(),
        prompts: prompts_of(r, agent),
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

/// Write `t` as the `text` member of `GET /api/stats/report`.
pub fn write_text(w: &mut Writer, t: &StatsText) {
    w.begin_obj();
    w.key("findings").begin_arr();
    for x in &t.findings {
        w.elem().begin_obj();
        w.key("detector");
        opt_text(w, x.detector.as_deref());
        w.key("impact").text(&x.impact);
        w.key("key").text(&x.key);
        w.key("note").text(&x.note);
        w.key("rule");
        opt_text(w, x.rule.as_deref());
        w.key("spread").text(&x.spread);
        w.key("table").str(x.table.as_str());
        w.key("title").text(&x.title);
        w.end_obj();
    }
    w.end_arr();
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
            table: WireTable::Fail,
            title: "title".into(),
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
