//! Error clusters: failed calls grouped by their masked signature.

use indexmap::IndexMap;

use crate::js::json_out::Writer;
use crate::js::regex::LazyRe;
use crate::js_re;
use crate::stats::accumulate::{
    DrillHit, DrillTable, DrillTarget, FinishCtx, SampleRef, Tally, hit_of, sample_line, sample_of,
    samples_of, write_samples,
};
use crate::stats::normalize::{cluster_key_of, details_of};
use crate::stats::rank::{
    asc, bump, clip, cmp_str, collapse_ws, desc, rank, round, top_key, uniq_sorted,
};
use crate::stats::redact::Redactor;
use crate::stats::schema::{CallFact, Fact};

const LABEL_MAX: usize = 120;
const DETAIL_KEYS: usize = 32;
const DETAIL_SHOWN: usize = 3;
const DRILL_CAP: usize = 500;
static TRAILING_PUNCT: LazyRe = js_re!(r"[:,\s]+$");

/// A one-line label for a signature: no trailing colon or comma.
pub fn cluster_label(sig: &str) -> String {
    clip(
        &TRAILING_PUNCT.replace_first(&collapse_ws(sig), |_| String::new()),
        LABEL_MAX,
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClusterRow {
    pub key: String,
    pub label: String,
    pub tool: String,
    pub rows: f64,
    pub sessions: f64,
    pub projects: f64,
    pub total_sec: f64,
    pub samples: Vec<SampleRef>,
}

impl ClusterRow {
    pub fn write(&self, w: &mut Writer) {
        w.begin_obj();
        w.key("key").text(&self.key);
        w.key("label").text(&self.label);
        w.key("tool").text(&self.tool);
        w.key("rows").num(self.rows);
        w.key("sessions").num(self.sessions);
        w.key("projects").num(self.projects);
        w.key("totalSec").num(self.total_sec);
        w.key("samples");
        write_samples(w, &self.samples);
        w.end_obj();
    }
}

#[derive(Default)]
struct Cluster {
    details: IndexMap<String, f64>,
    tally: Tally,
    tools: IndexMap<String, f64>,
}

fn detail_suffix(details: &IndexMap<String, f64>) -> String {
    let mut keys = uniq_sorted(details.keys().map(String::as_str));
    let n = |k: &str| details.get(k).copied().unwrap_or(0.0);
    keys.sort_by(|a, b| desc(n(a), n(b)));
    keys.truncate(DETAIL_SHOWN);
    if keys.is_empty() {
        String::new()
    } else {
        format!(" — {}", keys.join(", "))
    }
}

#[derive(Default)]
pub struct ClusterTable {
    wanted: Option<String>,
    clusters: IndexMap<String, Cluster>,
    drilled: Vec<CallFact>,
}

impl ClusterTable {
    pub fn new(drill: Option<&DrillTarget>) -> Self {
        Self {
            wanted: drill
                .filter(|d| d.table == DrillTable::Cluster)
                .map(|d| d.key.clone()),
            ..Self::default()
        }
    }

    pub fn add(&mut self, fact: &Fact, redact: &Redactor) {
        let Fact::Call(f) = fact else { return };
        let Some(sig) = f.sig.as_deref().filter(|s| !s.is_empty()) else {
            return;
        };
        let key = cluster_key_of(sig, &|s| redact.text(s));
        let c = self.clusters.entry(key.clone()).or_default();
        c.tally.add(f, Some(sample_of(f, sample_line(sig))));
        bump(&mut c.tools, &f.tool, 1.0);
        for d in details_of(sig) {
            if c.details.len() < DETAIL_KEYS || c.details.contains_key(&d) {
                bump(&mut c.details, &d, 1.0);
            }
        }
        if self.wanted.as_deref() == Some(key.as_str()) && self.drilled.len() < DRILL_CAP {
            self.drilled.push(f.clone());
        }
    }

    pub fn finish(&self, ctx: &mut FinishCtx<'_>) -> Vec<ClusterRow> {
        let rows: Vec<ClusterRow> = self
            .clusters
            .iter()
            .map(|(key, c)| ClusterRow {
                key: key.clone(),
                label: ctx.redact.line_max(
                    &format!("{}{}", cluster_label(key), detail_suffix(&c.details)),
                    LABEL_MAX,
                ),
                tool: top_key(&c.tools),
                rows: c.tally.rows,
                sessions: c.tally.session_count(),
                projects: c.tally.project_count(),
                total_sec: round(c.tally.total_sec),
                samples: samples_of(&c.tally.samples, ctx),
            })
            .collect();
        let ranked = rank(
            rows,
            |a, b| {
                desc(a.rows, b.rows)
                    .then_with(|| desc(a.sessions, b.sessions))
                    .then_with(|| cmp_str(&a.key, &b.key))
            },
            ctx.limit,
        );
        if ranked.dropped > 0 {
            ctx.caveat(format!(
                "{} of {} error clusters not shown",
                ranked.dropped, ranked.total
            ));
        }
        ranked.rows
    }

    pub fn drill(&self, ctx: &FinishCtx<'_>) -> Vec<DrillHit> {
        let mut facts: Vec<&CallFact> = self.drilled.iter().collect();
        facts.sort_by(|a, b| {
            cmp_str(a.ts.as_deref().unwrap_or(""), b.ts.as_deref().unwrap_or(""))
                .then_with(|| asc(a.seq, b.seq))
        });
        facts
            .into_iter()
            .map(|f| hit_of(f, ctx.redact.line(f.sig.as_deref().unwrap_or(""))))
            .collect()
    }
}
