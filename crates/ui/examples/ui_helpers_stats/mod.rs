//! The probe's stats questions: number spelling, a report's core text and
//! row texts, and a drill page's hit labels, on a HOME the harness names.

use std::collections::HashMap;

use peektrace_core::AgentRoots;
use peektrace_core::PathStyle;
use peektrace_core::js::time::Utc;
use peektrace_core::roots::{self, RootsEnv};
use peektrace_core::stats::accumulate::DrillTable as CoreTable;
use peektrace_core::stats::cache::{StatsCache, cache_root};
use peektrace_core::stats::service::{DrillPage, ReportRequest, StatsEnv, drill, report};
use peektrace_protocol::stats::{DrillView, SampleRefView, StatsReportView};
use peektrace_protocol::{AgentId, DrillTable};
use peektrace_ui::model::format::fmt_k;
use peektrace_ui::model::stats::chips::{
    SessionTitles, chip_label_of, drill_label, session_chips, target_label, title_of,
};
use peektrace_ui::model::stats::derive::{core_agent, text_of};
use peektrace_ui::model::stats::format::{at, hms, n, pct, rate_text};
use peektrace_ui::model::stats::rows::{
    self, RankRow, cause_badge, cause_cells, cause_learned, cause_named, cause_narrow, cause_pairs,
    cause_split, match_lines,
};
use peektrace_ui::model::stats::{Prompts, StatsText};
use serde_json::{Value, json};

fn nums(v: &Value) -> Vec<f64> {
    v.as_array()
        .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect())
        .unwrap_or_default()
}

pub fn fmt_answer(req: &Value) -> Value {
    let xs = nums(&req["xs"]);
    let rates: Vec<String> = req["rates"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|p| rate_text(p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0)))
                .collect()
        })
        .unwrap_or_default();
    json!({
        "hms": xs.iter().map(|x| hms(*x)).collect::<Vec<_>>(),
        "pct": xs.iter().map(|x| pct(*x)).collect::<Vec<_>>(),
        "n": xs.iter().map(|x| n(*x)).collect::<Vec<_>>(),
        "fmtK": xs.iter().map(|x| fmt_k(*x)).collect::<Vec<_>>(),
        "rate": rates,
        "at": nums(&req["ms"]).iter().map(|m| at(*m, 0.0)).collect::<Vec<_>>(),
        "days": req["days"].as_array().map(|a| a.iter().map(|d| rows::days_of(d.as_str())).collect::<Vec<_>>()),
        "agent": req["agents"].as_array().map(|a| a.iter().map(|d| rows::agent_of(d.as_str()).map(AgentId::as_str)).collect::<Vec<_>>()),
        "drill": req["drills"].as_array().map(|a| a.iter().map(|d| rows::parse_drill(d.as_str()).map(|(t, k)| json!({"table": rows::table_name(t), "key": k}))).collect::<Vec<_>>()),
    })
}

struct Setup {
    roots: AgentRoots,
    cache: String,
    now: f64,
    req: ReportRequest,
    agent: Option<AgentId>,
}

fn setup(req: &Value) -> Setup {
    let home = req["home"].as_str().unwrap_or_default().to_owned();
    let dir = req["dir"].as_str().map(str::to_owned);
    let env = RootsEnv::from_vars(home, PathStyle::NATIVE, |k| {
        (k == "PEEKTRACE_DIR").then(|| dir.clone()).flatten()
    });
    let settings = std::fs::read(env.settings_path())
        .ok()
        .map(|b| roots::parse_settings(&String::from_utf8_lossy(&b)))
        .unwrap_or_default();
    let agent = req["agent"].as_str().and_then(AgentId::parse);
    Setup {
        roots: AgentRoots::resolve(&env, &settings),
        cache: cache_root(&env),
        now: req["now"].as_f64().unwrap_or(0.0),
        req: ReportRequest {
            agents: agent.map(|a| vec![core_agent(a)]),
            days: req["days"].as_f64(),
            force: false,
            limit: None,
        },
        agent,
    }
}

fn env_of(s: &Setup) -> StatsEnv<'_> {
    StatsEnv {
        roots: &s.roots,
        cache: StatsCache::new(s.cache.clone()),
        local: &Utc,
        now_ms: s.now,
        progress: None,
    }
}

fn titles_of(req: &Value) -> SessionTitles {
    req["titles"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

fn map(m: &HashMap<String, String>) -> Value {
    json!(m)
}

fn prompts(p: &Prompts) -> Value {
    json!({"report": p.report, "cause": map(&p.cause), "fail": map(&p.fail), "time": map(&p.time)})
}

fn text_json(t: &StatsText) -> Value {
    json!({
        "findings": t.findings.iter().map(|f| json!({
            "detector": f.detector, "impact": f.impact, "key": f.key, "note": f.note,
            "rule": f.rule, "spread": f.spread, "table": rows::table_name(f.table), "title": f.title,
        })).collect::<Vec<_>>(),
        "suggested": t.suggested,
        "prompts": prompts(&t.prompts),
        "ruleLines": map(&t.rule_lines),
        "suggestedRule": map(&t.suggested_rule),
        "timeNotes": map(&t.time_notes),
        "generated": t.generated,
    })
}

fn rank(rows: &[RankRow], max: f64) -> Value {
    json!(
        rows.iter()
            .map(|r| json!({
                "key": r.key, "label": r.label, "valueText": r.value_text,
                "secondary": r.secondary, "width": rows::width_of(r.value, max),
            }))
            .collect::<Vec<_>>()
    )
}

fn chips(samples: &[SampleRefView], titles: &SessionTitles) -> Value {
    let groups: Vec<Value> = session_chips(samples)
        .iter()
        .map(|g| {
            let c = chip_label_of(g[0], titles);
            json!({
                "label": c.label, "id": c.id, "project": c.project, "session": c.session,
                "chipSubagent": c.subagent,
                "subagent": g.iter().any(|s| s.parent.is_some()),
                "title": title_of(g),
                "targets": g.iter().map(|s| target_label(s)).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({"groups": groups, "line": samples.first().map(|s| s.line.clone())})
}

fn view_json(r: &StatsReportView, titles: &SessionTitles) -> Value {
    let w = &r.waiting;
    json!({
        "corpus": rows::corpus_line(&r.corpus),
        "window": rows::window_line(r, &at(r.generated_at, 0.0)),
        "caveats": rows::caveats_line(r),
        "causes": r.causes.iter().map(|c| {
            let x = cause_cells(c);
            json!({
                "key": c.key, "badge": cause_badge(&c.key),
                "cells": [x.hits, x.retries, x.retry_sessions, x.hits_split, x.retry_time, x.ctx],
                "narrow": cause_narrow(c), "split": cause_split(c), "learned": cause_learned(c),
                "named": cause_named(c), "pairs": cause_pairs(c), "matches": match_lines(c),
                "detector": rows::detector_of(r, DrillTable::Cause, &c.key),
                "chips": chips(&c.samples, titles),
            })
        }).collect::<Vec<_>>(),
        "fails": r.fails.iter().map(|f| json!({
            "key": f.key,
            "cells": [n(f.sessions), n(f.rows), n(f.flagged), n(f.projects)],
            "detector": rows::detector_of(r, DrillTable::Fail, &f.key),
            "chips": chips(&f.samples, titles),
        })).collect::<Vec<_>>(),
        "time": r.time.iter().map(|t| json!({
            "key": t.key, "badges": rows::time_badges(t), "cells": rows::time_cells(t),
            "coPresent": rows::co_present_note(t), "bg": rows::backgrounded(t),
            "chips": chips(&t.samples, titles),
        })).collect::<Vec<_>>(),
        "wait": {
            "figures": rows::wait_figures(w).iter().map(|(v, l)| [v.clone(), (*l).to_owned()]).collect::<Vec<_>>(),
            "rows": rank(&rows::shape_rows(w), w.total_sec),
            "note": rows::wait_note(w),
        },
        "clusters": {
            "rows": rank(&rows::cluster_rows(&r.clusters), rows::cluster_max(&r.clusters)),
            "more": rows::more_clusters(&r.clusters),
        },
    })
}

pub fn stats_answer(req: &Value) -> Value {
    let s = setup(req);
    let r = match report(&env_of(&s), &s.req) {
        Ok(r) => r,
        Err(e) => return json!({"error": format!("{e:?}")}),
    };
    let body = r.to_json(false);
    let Ok(view) = serde_json::from_str::<StatsReportView>(&body) else {
        return json!({"error": "decode"});
    };
    let titles = titles_of(req);
    let text = text_of(&r, s.agent, 0.0);
    json!({
        "report": serde_json::from_str::<Value>(&body).unwrap_or(Value::Null),
        "text": text_json(&text),
        "view": view_json(&view, &titles),
        "showMore": rows::show_more(text.findings.len()),
        "retryWindow": peektrace_ui::model::stats::RETRY_WINDOW,
        "findingChips": text.findings.iter().map(|f| {
            let samples = match f.table {
                DrillTable::Cause => view.causes.iter().find(|c| c.key == f.key).map(|c| c.samples.clone()),
                DrillTable::Fail => view.fails.iter().find(|c| c.key == f.key).map(|c| c.samples.clone()),
                _ => view.time.iter().find(|c| c.key == f.key).map(|c| c.samples.clone()),
            }.unwrap_or_default();
            chips(&samples, &titles)
        }).collect::<Vec<_>>(),
    })
}

fn core_table(t: &str) -> Option<CoreTable> {
    CoreTable::parse(t)
}

pub fn drill_answer(req: &Value) -> Value {
    let s = setup(req);
    let Some(table) = req["table"].as_str().and_then(core_table) else {
        return json!({"error": "table"});
    };
    let key = req["key"].as_str().unwrap_or_default();
    let page = DrillPage {
        offset: req["offset"].as_f64(),
        page_size: Some(rows::DRILL_PAGE),
    };
    let d = match drill(&env_of(&s), &s.req, table, key, page) {
        Ok(d) => d,
        Err(e) => return json!({"error": format!("{e:?}")}),
    };
    let body = d.to_json(false);
    let Ok(view) = serde_json::from_str::<DrillView>(&body) else {
        return json!({"error": "decode"});
    };
    let titles = titles_of(req);
    let p = rows::page_note(view.offset, view.hits.len(), view.total);
    json!({
        "drill": serde_json::from_str::<Value>(&body).unwrap_or(Value::Null),
        "hits": view.hits.iter().map(|h| json!({
            "label": drill_label(h, &titles),
            "dur": h.dur_sec.map_or_else(|| "—".to_owned(), hms),
            "meta": format!("{} · {} · {}", drill_label(h, &titles), h.sample.kind, h.sample.ts.as_deref().unwrap_or("no timestamp")),
            "title": format!("{} · {}", h.sample.project, h.sample.parent.as_deref().unwrap_or(&h.sample.sid)),
        })).collect::<Vec<_>>(),
        "note": p.note,
        "more": p.more.map(|(l, next)| json!([l, next])),
        "back": p.back,
    })
}
