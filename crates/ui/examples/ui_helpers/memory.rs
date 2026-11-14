//! The memory questions of the probe: explorer rules on vaults, fixed
//! sweeps (sizes, dates, the write gate, error text, body splits) and the
//! chart geometry the inspector draws as SVG.

use peektrace_core::js::system_zone::SystemZone;
use peektrace_js::num::js_round;
use peektrace_protocol::view::SessionHeader;
use peektrace_protocol::view::memory::{
    AllVaultsView, BudgetView, EntryView, GraphView, LinkView, ProjectLabelView, TypeCounts,
};
use peektrace_protocol::{ApiError, CapabilitySupport};
use peektrace_ui::chart::memory::{DONUT, budget_bars, donut, donut_radii, ring};
use peektrace_ui::model::memory::{
    self as m, BodyPart, Sort, SortKey, body_parts, crud_verdict, fmt_date, fmt_size,
    project_options, sort_entries, support_summary, title_of, visible_vaults,
};
use serde_json::{Value, json};

fn decode<T: serde::de::DeserializeOwned + Default>(v: &Value) -> T {
    serde_json::from_value(v.clone()).unwrap_or_default()
}

fn strs(v: &Value) -> Vec<String> {
    decode(v)
}

fn parts_json(e: &EntryView) -> Vec<Value> {
    body_parts(e)
        .into_iter()
        .map(|p| match p {
            BodyPart::Text(t) => json!({ "text": t }),
            BodyPart::Link { text, slug } => json!({ "link": text, "slug": slug }),
        })
        .collect()
}

fn vault_answer(v: &peektrace_protocol::view::memory::VaultView, zone: &SystemZone) -> Value {
    let refs: Vec<&EntryView> = v.entries.iter().collect();
    let sorts: serde_json::Map<String, Value> = SortKey::ALL
        .into_iter()
        .flat_map(|key| [true, false].map(|asc| (key, asc)))
        .map(|(key, asc)| {
            let order: Vec<&str> = sort_entries(&refs, Sort { key, asc })
                .iter()
                .map(|e| e.slug.as_str())
                .collect();
            (format!("{}:{asc}", key.as_str()), json!(order))
        })
        .collect();
    json!({
        "slug": v.slug,
        "sorts": sorts,
        "titles": v.entries.iter().map(title_of).collect::<Vec<_>>(),
        "sizes": v.entries.iter().map(|e| fmt_size(e.size)).collect::<Vec<_>>(),
        "dates": v.entries.iter().map(|e| fmt_date(&e.mtime, zone)).collect::<Vec<_>>(),
        "parts": v.entries.iter().map(parts_json).collect::<Vec<_>>(),
        "chips": v.entries.iter().map(|e| e.links.iter().map(|l| [&l.raw_target, &l.target_slug]).collect::<Vec<_>>()).collect::<Vec<_>>(),
    })
}

/// Explorer rules over one set of vaults.
pub fn rules(req: &Value) -> Value {
    let all: AllVaultsView = decode(&req["all"]);
    let headers: Vec<SessionHeader> = decode(&req["headers"]);
    let labels: Vec<ProjectLabelView> = decode(&req["labels"]);
    let zone = SystemZone::named(req["tz"].as_str().unwrap_or("UTC"));
    let options: Vec<Value> = project_options(&headers, &labels)
        .into_iter()
        .map(|o| json!([o.slug, o.label, o.last_active]))
        .collect();
    let filters: Vec<Value> = req["cases"]
        .as_array()
        .map(|cs| {
            cs.iter()
                .map(|c| {
                    let s = |k: &str| c[k].as_str().unwrap_or_default().to_owned();
                    visible_vaults(&all, &s("scope"), &s("query"), &s("type"))
                        .iter()
                        .map(|v| {
                            let slugs: Vec<&str> =
                                v.entries.iter().map(|e| e.slug.as_str()).collect();
                            json!([v.vault.slug, slugs])
                        })
                        .collect::<Vec<_>>()
                })
                .map(Value::from)
                .collect()
        })
        .unwrap_or_default();
    json!({
        "options": options,
        "filters": filters,
        "vaults": all.vaults.iter().map(|v| vault_answer(v, &zone)).collect::<Vec<_>>(),
    })
}

fn cells(v: &Value) -> m::CrudCells {
    let cell = |i: usize| -> Option<CapabilitySupport> {
        serde_json::from_value(v["cells"][i].clone()).ok()
    };
    if v["row"].as_bool() == Some(false) {
        return [None, None, None, None];
    }
    [cell(0), cell(1), cell(2), cell(3)]
}

/// Sweeps that need no vault.
pub fn fixed(req: &Value) -> Value {
    let zone = SystemZone::named(req["tz"].as_str().unwrap_or("UTC"));
    let sizes: Vec<u64> = decode(&req["sizes"]);
    let caps: Vec<Value> = decode(&req["caps"]);
    let errors: Vec<ApiError> = req["errors"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| serde_json::from_value(e.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    let bodies: Vec<Value> = req["bodies"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|b| {
                    let links: Vec<(String, String)> = decode(&b["links"]);
                    let e = EntryView {
                        slug: "x".into(),
                        file_name: "x.md".into(),
                        name: None,
                        description: None,
                        kind: None,
                        body: b["body"].as_str().unwrap_or_default().to_owned(),
                        size: 0,
                        lines: 0,
                        mtime: String::new(),
                        mtime_ms: 0.0,
                        in_index: false,
                        links: links
                            .into_iter()
                            .map(|(raw_target, target_slug)| LinkView {
                                kind: "wiki".into(),
                                raw_target,
                                target_slug,
                                line: 1,
                            })
                            .collect(),
                    };
                    Value::from(parts_json(&e))
                })
                .collect()
        })
        .unwrap_or_default();
    json!({
        "sizes": sizes.into_iter().map(fmt_size).collect::<Vec<_>>(),
        "dates": strs(&req["dates"]).iter().map(|d| fmt_date(d, &zone)).collect::<Vec<_>>(),
        "summary": caps.iter().map(|c| support_summary(&cells(c))).collect::<Vec<_>>(),
        "verdicts": caps.iter().map(|c| {
            let v = crud_verdict(&cells(c), c["readOnly"].as_bool().unwrap_or(false));
            json!([m::level_str(v.level), v.note, v.blocked_by.map(m::Blocked::as_str)])
        }).collect::<Vec<_>>(),
        "errors": errors.iter().map(m::wire_error_message).collect::<Vec<_>>(),
        "bodies": bodies,
        "types": m::type_filter_options(),
    })
}

fn r6(x: f64) -> f64 {
    js_round(x * 1e6) / 1e6
}

fn r3(x: f64) -> f64 {
    js_round(x * 1000.0) / 1000.0
}

/// A point on d3's arc: angle from 12 o'clock, read as d3-shape does.
fn d3_point(r: f64, a: f64) -> [f64; 2] {
    let t = a - std::f64::consts::FRAC_PI_2;
    [r3(0.0 + r * t.cos()), r3(0.0 + r * t.sin())]
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct GeomReq {
    graphs: Vec<GraphView>,
    counts: Vec<TypeCounts>,
    budgets: Vec<BudgetView>,
}

/// Ring, donut and bars, as the inspector's SVG and gauge show them. Read
/// from the raw line: a `Value` would sort the type counts' keys.
pub fn geom(line: &str) -> Value {
    let GeomReq {
        graphs,
        counts,
        budgets,
    } = serde_json::from_str(line).unwrap_or_default();
    let rings: Vec<Value> = graphs
        .iter()
        .map(|g| {
            if g.nodes.is_empty() {
                return json!({ "empty": true });
            }
            let r = ring(g);
            json!({
                "nodes": r.nodes.iter().map(|n| json!([n.slug, r6(n.x), r6(n.y), r6(n.r), n.in_index, r6(n.label[1])])).collect::<Vec<_>>(),
                "edges": r.edges.iter().map(|e| json!([r6(e.from[0]), r6(e.from[1]), r6(e.to[0]), r6(e.to[1]), e.resolved])).collect::<Vec<_>>(),
            })
        })
        .collect();
    let (outer, inner) = donut_radii(DONUT);
    let donuts: Vec<Value> = counts
        .iter()
        .map(|c| {
            donut(c)
                .iter()
                .map(|s| {
                    let pts = [
                        d3_point(outer, s.start),
                        d3_point(outer, s.end),
                        d3_point(inner, s.end),
                        d3_point(inner, s.start),
                    ];
                    json!([s.kind, s.chart, pts])
                })
                .collect::<Vec<_>>()
        })
        .map(Value::from)
        .collect();
    let bars: Vec<Value> = budgets
        .iter()
        .map(|b| {
            let bars: Vec<Value> = budget_bars(b)
                .iter()
                .map(|x| json!([x.label, x.text, x.pct, x.over, x.tone.as_str()]))
                .collect();
            let fold = (b.kind != "absent" && b.below_fold_count > 0)
                .then(|| m::below_fold_text(b.below_fold_count));
            json!({ "absent": b.kind == "absent", "bars": bars, "belowFold": fold })
        })
        .collect();
    json!({ "rings": rings, "donuts": donuts, "bars": bars })
}
