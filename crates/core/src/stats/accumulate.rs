//! What every detector shares: sample refs, drill hits, per-key tallies,
//! the finish context, and the corpus totals sink.

use std::collections::HashSet;

use indexmap::IndexMap;

use super::rank::{
    SAMPLE_POOL_LINE_MAX, SampleKey, bound_pool, clip, cmp_str, collapse_ws, pick_samples, round,
};
use super::redact::Redactor;
use super::schema::{CallFact, Fact, Kind, WriteExt};
use crate::js::json_out::Writer;
use crate::model::Agent;

pub const HUMAN_WAIT_TOOLS: [&str; 4] = [
    "AskUserQuestion",
    "ExitPlanMode",
    "SendUserMessage",
    "TaskOutput",
];

/// The table a drill reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrillTable {
    Cause,
    Fail,
    Time,
    Cluster,
    Wait,
}

impl DrillTable {
    pub const ALL: [Self; 5] = [
        Self::Cause,
        Self::Fail,
        Self::Time,
        Self::Cluster,
        Self::Wait,
    ];

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cause => "cause",
            Self::Fail => "fail",
            Self::Time => "time",
            Self::Cluster => "cluster",
            Self::Wait => "wait",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrillTarget {
    pub table: DrillTable,
    pub key: String,
}

/// A pointer from an aggregate row back to one call.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleRef {
    pub sid: String,
    pub agent: Agent,
    pub project: String,
    pub kind: Kind,
    pub parent: Option<String>,
    pub seq: f64,
    pub tool_use_id: Option<String>,
    pub ts: Option<String>,
    pub line: String,
}

impl SampleKey for SampleRef {
    fn line(&self) -> &str {
        &self.line
    }
    fn sid(&self) -> &str {
        &self.sid
    }
    fn seq(&self) -> f64 {
        self.seq
    }
}

impl SampleRef {
    pub fn write_fields(&self, w: &mut Writer) {
        w.key("sid").text(&self.sid);
        w.key("agent").str(self.agent.as_str());
        w.key("project").text(&self.project);
        w.key("kind").str(self.kind.as_str());
        w.key("parent").opt_text(self.parent.as_deref());
        w.key("seq").num(self.seq);
        w.key("toolUseId").opt_text(self.tool_use_id.as_deref());
        w.key("ts").opt_text(self.ts.as_deref());
        w.key("line").text(&self.line);
    }

    pub fn write(&self, w: &mut Writer) {
        w.begin_obj();
        self.write_fields(w);
        w.end_obj();
    }
}

/// One call behind a drilled row.
#[derive(Debug, Clone, PartialEq)]
pub struct DrillHit {
    pub sample: SampleRef,
    pub dur_sec: Option<f64>,
    pub tool: String,
    pub failed: bool,
    pub pair_tool_use_id: Option<String>,
    pub pair_line: Option<String>,
}

impl DrillHit {
    pub fn write(&self, w: &mut Writer) {
        w.begin_obj();
        self.sample.write_fields(w);
        w.key("durSec").opt_num(self.dur_sec);
        w.key("tool").text(&self.tool);
        w.key("failed").bool(self.failed);
        w.key("pairToolUseId")
            .opt_text(self.pair_tool_use_id.as_deref());
        w.key("pairLine").opt_text(self.pair_line.as_deref());
        w.end_obj();
    }
}

pub fn write_samples(w: &mut Writer, samples: &[SampleRef]) {
    w.begin_arr();
    for s in samples {
        w.elem();
        s.write(w);
    }
    w.end_arr();
}

/// A fact's text as a sample line: collapsed and clipped, not yet re-redacted.
pub fn sample_line(s: &str) -> String {
    clip(&collapse_ws(s), SAMPLE_POOL_LINE_MAX)
}

pub fn sample_of(fact: &CallFact, line: String) -> SampleRef {
    SampleRef {
        sid: fact.base.sid.clone(),
        agent: fact.base.agent,
        project: fact.base.project.clone(),
        kind: fact.base.kind,
        parent: fact.base.parent.clone(),
        seq: fact.seq,
        tool_use_id: fact.tool_use_id.clone(),
        ts: fact.ts.clone(),
        line,
    }
}

pub fn hit_of(fact: &CallFact, line: String) -> DrillHit {
    DrillHit {
        sample: sample_of(fact, line),
        dur_sec: fact.dur_sec,
        tool: fact.tool.clone(),
        failed: fact.failed,
        pair_tool_use_id: None,
        pair_line: None,
    }
}

/// Per-key state every ranked table needs.
#[derive(Debug, Default)]
pub struct Tally {
    pub rows: f64,
    pub flagged: f64,
    pub total_sec: f64,
    pub durations: Vec<f64>,
    pub sessions: HashSet<String>,
    pub projects: HashSet<String>,
    pub samples: Vec<SampleRef>,
}

impl Tally {
    pub fn add(&mut self, fact: &CallFact, sample: Option<SampleRef>) {
        self.rows += 1.0;
        if fact.err {
            self.flagged += 1.0;
        }
        if let Some(d) = fact.dur_sec {
            self.total_sec += d;
            self.durations.push(d);
        }
        if !self.sessions.contains(&fact.base.sid) {
            self.sessions.insert(fact.base.sid.clone());
        }
        if !self.projects.contains(&fact.base.project) {
            self.projects.insert(fact.base.project.clone());
        }
        if let Some(s) = sample {
            self.samples.push(s);
            bound_pool(&mut self.samples);
        }
    }

    #[expect(clippy::cast_precision_loss)]
    pub fn session_count(&self) -> f64 {
        self.sessions.len() as f64
    }

    #[expect(clippy::cast_precision_loss)]
    pub fn project_count(&self) -> f64 {
        self.projects.len() as f64
    }
}

/// The sample lines a row ships: picked, then redacted once more.
pub fn samples_of(pool: &[SampleRef], ctx: &FinishCtx<'_>) -> Vec<SampleRef> {
    pick_samples(pool, ctx.sample_cap)
        .into_iter()
        .map(|s| SampleRef {
            line: ctx.redact.line(&s.line),
            ..s
        })
        .collect()
}

pub fn redact_hits(hits: &[DrillHit], redact: &Redactor) -> Vec<DrillHit> {
    hits.iter()
        .map(|h| DrillHit {
            sample: SampleRef {
                line: redact.line(&h.sample.line),
                ..h.sample.clone()
            },
            pair_line: h.pair_line.as_deref().map(|l| redact.line(l)),
            ..h.clone()
        })
        .collect()
}

/// The report window.
#[derive(Debug, Clone, PartialEq)]
pub struct StatsWindow {
    pub days: f64,
    pub from_iso: String,
    pub to_iso: String,
}

/// Corpus totals every share divides by.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CorpusSummary {
    pub transcripts: f64,
    pub sessions: f64,
    pub projects: f64,
    pub tool_calls: f64,
    pub bash_calls: f64,
    pub bash_sec: f64,
    pub tool_sec: f64,
    pub human_wait_sec: f64,
    pub kinds: Vec<(String, f64)>,
    pub agents: Vec<(String, f64)>,
}

/// What a detector reads when it produces rows.
pub struct FinishCtx<'a> {
    pub corpus: &'a CorpusSummary,
    pub window: &'a StatsWindow,
    pub redact: &'a Redactor,
    pub limit: f64,
    pub sample_cap: usize,
    pub caveats: Vec<String>,
}

impl FinishCtx<'_> {
    pub fn caveat(&mut self, note: String) {
        self.caveats.push(note);
    }
}

fn sorted_record(map: IndexMap<String, f64>) -> Vec<(String, f64)> {
    let mut v: Vec<(String, f64)> = map.into_iter().collect();
    v.sort_by(|a, b| cmp_str(&a.0, &b.0));
    v
}

/// The spine's own sink: corpus totals.
#[derive(Debug, Default)]
pub struct CorpusTotals {
    projects: HashSet<String>,
    sessions: HashSet<String>,
    kinds: IndexMap<String, f64>,
    agents: IndexMap<String, f64>,
    transcripts: f64,
    tool_calls: f64,
    bash_calls: f64,
    bash_sec: f64,
    tool_sec: f64,
    human_wait_sec: f64,
}

impl CorpusTotals {
    pub fn add(&mut self, fact: &Fact) {
        match fact {
            Fact::Session(s) => {
                self.transcripts += 1.0;
                self.sessions.insert(s.base.sid.clone());
                self.projects.insert(s.base.project.clone());
                *self
                    .kinds
                    .entry(s.base.kind.as_str().to_owned())
                    .or_default() += 1.0;
                *self
                    .agents
                    .entry(s.base.agent.as_str().to_owned())
                    .or_default() += 1.0;
            }
            Fact::Call(c) => {
                self.tool_calls += 1.0;
                let sec = c.dur_sec.unwrap_or(0.0);
                if HUMAN_WAIT_TOOLS.contains(&c.tool.as_str()) {
                    self.human_wait_sec += sec;
                    return;
                }
                self.tool_sec += sec;
                if c.is_bash() {
                    self.bash_calls += 1.0;
                    self.bash_sec += sec;
                }
            }
            Fact::Usage(_) => {}
        }
    }

    #[expect(clippy::cast_precision_loss)]
    pub fn finish(self) -> CorpusSummary {
        CorpusSummary {
            transcripts: self.transcripts,
            sessions: self.sessions.len() as f64,
            projects: self.projects.len() as f64,
            tool_calls: self.tool_calls,
            bash_calls: self.bash_calls,
            bash_sec: round(self.bash_sec),
            tool_sec: round(self.tool_sec),
            human_wait_sec: round(self.human_wait_sec),
            kinds: sorted_record(self.kinds),
            agents: sorted_record(self.agents),
        }
    }
}

impl CorpusSummary {
    pub fn write(&self, w: &mut Writer) {
        w.begin_obj();
        for (k, v) in [
            ("transcripts", self.transcripts),
            ("sessions", self.sessions),
            ("projects", self.projects),
            ("toolCalls", self.tool_calls),
            ("bashCalls", self.bash_calls),
            ("bashSec", self.bash_sec),
            ("toolSec", self.tool_sec),
            ("humanWaitSec", self.human_wait_sec),
        ] {
            w.key(k).num(v);
        }
        for (k, rec) in [("kinds", &self.kinds), ("agents", &self.agents)] {
            w.key(k).begin_obj();
            for (rk, rv) in rec {
                w.key_text(rk).num(*rv);
            }
            w.end_obj();
        }
        w.end_obj();
    }
}
