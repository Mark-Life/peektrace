//! Parity probe for the app's pure helpers: one JSON request per stdin
//! line, one JSON answer per stdout line. `scripts/rust/parity-ui.ts`
//! asks the TS inspector the same questions and diffs the answers.

use std::io::{BufRead as _, Write as _};

use peektrace_protocol::AgentId;
use peektrace_protocol::view::{AnalyzedSession, EventKind, SessionHeader};
use peektrace_ui::chart::geom::{Growth, growth, pos};
use peektrace_ui::chart::hit::{ClickAction, click_action, snap_index_at_x};
use peektrace_ui::model::chat::{
    ChatNode, Lane, build_chat_plan, chat_ids_of, chat_node_keys, chip_label, lane_of, needs_clamp,
    visible_rows,
};
use peektrace_ui::model::format::{first_line, fmt, fmt_bytes, fmt_k, fmt_pct, zone_of};
use peektrace_ui::model::jsnum::to_fixed;
use peektrace_ui::model::labels::{analyzed_title, cost_summary, provider_label};
use peektrace_ui::model::list::{self, Filters};
use peektrace_ui::model::tools::{
    Body, ToolPane, body_view, call_view, index_tools, result_pane, tool_name_of, tool_state,
};
use peektrace_ui::model::turns::{cross_event_index, turn_numbers};
use peektrace_ui::model::utf16;
use serde_json::{Value, json};

/// Tool bodies past this many UTF-16 units are skipped, as the TS side does.
const BODY_MAX: usize = 200_000;

fn filters(v: &Value) -> Filters {
    let s = |k: &str| v[k].as_str().filter(|s| *s != "all").map(str::to_owned);
    Filters {
        query: v["query"].as_str().unwrap_or_default().to_owned(),
        agent: v["agent"].as_str().and_then(AgentId::parse),
        project: s("project"),
        branch: s("branch"),
        model: s("model"),
        source: s("source"),
        range: None,
    }
}

fn list_answer(req: &Value) -> Value {
    let headers: Vec<SessionHeader> =
        serde_json::from_value(req["headers"].clone()).unwrap_or_default();
    let now = req["now"].as_f64().unwrap_or(0.0);
    let per: Vec<Value> = headers
        .iter()
        .map(|h| {
            json!({
                "stamp": list::fmt_stamp(Some(&h.updated_at)),
                "stampTitle": list::stamp_title(h),
                "title": list::title_of(h),
                "projectLabel": list::project_label(h),
                "projectKey": list::project_key(h),
                "branch": list::visible_branch(h.git_branch.as_deref()),
                "live": list::is_live(h, now),
                "days": list::active_days(h),
            })
        })
        .collect();
    let order: Vec<&str> = list::sort_by_recent(&headers, now)
        .into_iter()
        .map(|i| headers[i].id.as_str())
        .collect();
    let options: Vec<Value> = list::project_options(&headers)
        .into_iter()
        .map(|o| json!([o.value, o.label]))
        .collect();
    let cases: Vec<Value> = req["filters"]
        .as_array()
        .map(|fs| {
            fs.iter()
                .map(|f| {
                    let f = filters(f);
                    let hits: Vec<bool> = headers.iter().map(|h| list::matches(h, &f)).collect();
                    let hiding: Vec<Vec<&str>> = headers
                        .iter()
                        .map(|h| {
                            list::hiding_filters(h, &f)
                                .into_iter()
                                .map(list::FilterName::as_str)
                                .collect()
                        })
                        .collect();
                    json!({ "matches": hits, "hiding": hiding, "active": list::active_count(&f) })
                })
                .collect()
        })
        .unwrap_or_default();
    json!({ "per": per, "order": order, "options": options, "cases": cases })
}

fn num_answer(req: &Value) -> Value {
    let xs: Vec<f64> = req["xs"]
        .as_array()
        .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect())
        .unwrap_or_default();
    let zones: Vec<&str> = req["zones"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|p| {
                    zone_of(p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0)).as_str()
                })
                .collect()
        })
        .unwrap_or_default();
    let lines: Vec<String> = req["lines"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| first_line(s.as_str().unwrap_or_default(), 160))
                .collect()
        })
        .unwrap_or_default();
    json!({
        "fmt": xs.iter().map(|x| fmt(*x)).collect::<Vec<_>>(),
        "fmtK": xs.iter().map(|x| fmt_k(*x)).collect::<Vec<_>>(),
        "fmtPct": xs.iter().map(|x| fmt_pct(*x)).collect::<Vec<_>>(),
        "fmtBytes": xs.iter().filter(|x| x.is_finite() && **x >= 0.0).map(|x| fmt_bytes(*x)).collect::<Vec<_>>(),
        "zones": zones,
        "firstLine": lines,
    })
}

fn hit_answer(req: &Value) -> Value {
    let f = |v: &Value| v.as_f64().unwrap_or(0.0);
    let snaps: Vec<i64> = req["snaps"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "small counts"
                    )]
                    let n = f(&c[1]) as usize;
                    snap_index_at_x(f(&c[0]), n, f(&c[2]), f(&c[3]))
                        .map_or(-1, |i| i64::try_from(i).unwrap_or(-1))
                })
                .collect()
        })
        .unwrap_or_default();
    let clicks: Vec<&str> = req["clicks"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| {
                    let armed = usize::try_from(c[0].as_i64().unwrap_or(-1)).ok();
                    let i = usize::try_from(c[1].as_i64().unwrap_or(0)).unwrap_or(0);
                    match click_action(armed, i, c[2].as_bool().unwrap_or(false)) {
                        ClickAction::Show => "show",
                        ClickAction::Jump => "jump",
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    json!({ "snaps": snaps, "clicks": clicks })
}

fn node(n: &ChatNode) -> Value {
    match n {
        ChatNode::Message(p) => json!(["message", p]),
        ChatNode::Band(p) => json!(["band", p]),
        ChatNode::Notice(p) => json!(["notice", p]),
        ChatNode::Tool { call, result } => json!(["tool", call, result]),
        ChatNode::Run(items) => json!([
            "run",
            items.iter().map(|i| i.pos).collect::<Vec<_>>(),
            items.iter().map(|i| i.repeat).collect::<Vec<_>>()
        ]),
        ChatNode::Turn(t) => json!(["turn", t]),
        ChatNode::DumbZone => json!(["dumbzone"]),
    }
}

fn pane(p: &ToolPane) -> Value {
    match p {
        ToolPane::Code {
            label,
            code,
            language,
        } => json!([label, "code", language, code, null, null]),
        ToolPane::Prose { label, text } => json!([label, "prose", null, text, null, null]),
        ToolPane::Diff {
            label,
            diff,
            language,
            ..
        } => json!([label, "diff", language, null, diff.added, diff.removed]),
    }
}

fn analysis_answer(a: &AnalyzedSession) -> Value {
    let turns = turn_numbers(a);
    let cross = cross_event_index(a);
    let rows = visible_rows(&a.events);
    let plan = build_chat_plan(&a.events, &rows, &turns, cross);
    let ix = index_tools(&a.events);
    let tools: Vec<Value> = a
        .events
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e.kind, EventKind::ToolCall | EventKind::ToolResult))
        .filter(|(_, e)| utf16::len(&e.body) <= BODY_MAX)
        .map(|(p, e)| {
            let mut out = json!({ "pos": p, "state": tool_state(&a.events, e, &ix).as_str(), "name": tool_name_of(&a.events, e, &ix) });
            if e.kind == EventKind::ToolCall {
                let v = call_view(e);
                out["summary"] = json!(v.summary);
                out["panes"] = v.panes.iter().map(pane).collect();
            } else {
                out["result"] = match result_pane(e) {
                    Body::Code { code, language } => json!(["code", language, code]),
                    Body::Prose { text } => json!(["prose", null, text]),
                };
            }
            out
        })
        .collect();
    let lane = |l: Lane| match l {
        Lane::User => "user",
        Lane::Agent => "agent",
        Lane::Center => "center",
    };
    json!({
        "title": analyzed_title(a),
        "turns": turns,
        "cross": cross.map_or(-1, |c| i64::try_from(c).unwrap_or(-1)),
        "plan": plan.iter().map(node).collect::<Vec<_>>(),
        "keys": chat_node_keys(&plan),
        "ids": chat_ids_of(&a.events, &rows),
        "lanes": a.events.iter().map(|e| lane(lane_of(e))).collect::<Vec<_>>(),
        "chips": a.events.iter().map(chip_label).collect::<Vec<_>>(),
        "clamp": a.events.iter().map(needs_clamp).collect::<Vec<_>>(),
        "bodyView": a.events.iter().map(body_view).collect::<Vec<_>>(),
        "tools": tools,
        "cost": a.cost.as_ref().map(|c| { let s = cost_summary(c, provider_label(&a.provider)); json!([s.total, s.notes]) }),
    })
}

fn n1(v: f64) -> String {
    to_fixed(v, 1)
}

fn d_of(points: impl Iterator<Item = [f64; 2]>) -> String {
    points
        .map(|p| format!("{} {}", n1(p[0]), n1(p[1])))
        .collect::<Vec<_>>()
        .join(" L ")
}

fn growth_answer(a: &AnalyzedSession, g: &Growth) -> Value {
    let areas: Vec<String> = g
        .bands
        .iter()
        .map(|b| {
            format!(
                "M {} L {} Z",
                d_of(b.top.iter().copied()),
                d_of(b.bottom.iter().rev().copied())
            )
        })
        .collect();
    let mut texts = vec![g.dz_label.clone()];
    if g.ceiling_y.is_some() {
        texts.push("200K-model ceiling".to_owned());
    }
    texts.extend(g.grid.iter().map(|(_, l)| l.clone()));
    texts.extend(g.ticks.iter().map(|(_, l)| l.clone()));
    if let Some(c) = &g.cross {
        texts.push(c.label.clone());
    }
    texts.push(g.peak.label.clone());
    let p = &g.peak;
    let peak = format!(
        "M {} {} L {} {} L {} {} L {} {} Z",
        n1(p.x),
        n1(p.y - 6.0),
        n1(p.x + 6.0),
        n1(p.y),
        n1(p.x),
        n1(p.y + 6.0),
        n1(p.x - 6.0),
        n1(p.y)
    );
    let shifted = usize::try_from(a.dumb_zone_cross_turn).is_ok_and(|c| pos(&a.snapshots, c) != c)
        || pos(&a.snapshots, a.peak_turn_index) != a.peak_turn_index
        || a.compaction_turns
            .iter()
            .any(|t| pos(&a.snapshots, *t) != *t);
    json!({
        "areas": areas,
        "silhouette": format!("M {}", d_of(g.silhouette.iter().copied())),
        "dots": g.dots.iter().map(|(x, y, r)| [n1(*x), n1(*y), n1(*r)]).collect::<Vec<_>>(),
        "grid": g.grid.iter().map(|(y, _)| n1(*y)).collect::<Vec<_>>(),
        "cliffs": g.cliffs.iter().map(|x| n1(*x)).collect::<Vec<_>>(),
        "dz": n1(g.dz_y),
        "texts": texts,
        "peak": peak,
        "aria": g.aria,
        "shifted": shifted,
    })
}

fn answer(req: &Value) -> Value {
    let load = || -> Option<AnalyzedSession> {
        let path = req["path"].as_str()?;
        let body = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&peektrace_protocol::view::mend_surrogates(&body)).ok()
    };
    match req["fn"].as_str().unwrap_or_default() {
        "list" => list_answer(req),
        "num" => num_answer(req),
        "hit" => hit_answer(req),
        "analysis" => load().map_or(Value::Null, |a| analysis_answer(&a)),
        "growth" => load().map_or(Value::Null, |a| {
            growth(&a).map_or(Value::Null, |g| growth_answer(&a, &g))
        }),
        other => json!({ "error": format!("unknown fn {other}") }),
    }
}

fn main() {
    let stdin = std::io::stdin();
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    for line in stdin.lock().lines().map_while(Result::ok) {
        let req: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let _ = writeln!(out, "{}", answer(&req));
    }
}
