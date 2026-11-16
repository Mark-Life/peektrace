//! Rows of the corpus panels and their JSON shapes.

use crate::js::json_out::Writer;
use crate::model::Agent;
use crate::pricing::{CostTokens, base_cost, card_for};
use crate::stats::accumulate::SampleRef;
use crate::stats::schema::Kind;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Tokens {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_creation: f64,
}

impl Tokens {
    pub(super) fn add(self, b: Self) -> Self {
        Self {
            input: self.input + b.input,
            output: self.output + b.output,
            cache_read: self.cache_read + b.cache_read,
            cache_creation: self.cache_creation + b.cache_creation,
        }
    }

    pub(super) fn map(self, f: impl Fn(f64) -> f64) -> Self {
        Self {
            input: f(self.input),
            output: f(self.output),
            cache_read: f(self.cache_read),
            cache_creation: f(self.cache_creation),
        }
    }

    pub(super) fn cost(self, model: &str) -> f64 {
        base_cost(
            card_for(model).as_ref(),
            &CostTokens {
                input: self.input,
                output: self.output,
                cache_read: self.cache_read,
                cache_creation: self.cache_creation,
                cache_creation_1h: None,
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextRow {
    pub key: String,
    pub sid: String,
    pub agent: Agent,
    pub project: String,
    pub kind: Kind,
    pub max_context_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub output_tokens: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FanoutRow {
    pub key: String,
    pub project: String,
    pub agents: f64,
    pub prefix_tokens: f64,
    pub tool_calls: f64,
    pub total_sec: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WebRepeatRow {
    pub key: String,
    pub tool: String,
    pub calls: f64,
    pub sessions: f64,
    pub chars: f64,
    pub same_run: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CostRow {
    pub key: String,
    pub label: String,
    pub sessions: f64,
    pub tokens: Tokens,
}

/// Subagents and workflow agents: what they cost, and the prompt prefix every
/// agent pays for again.
#[derive(Debug, Clone, PartialEq)]
pub struct FanoutInsight {
    pub usd: f64,
    pub agents: f64,
    /// Main sessions that spawned them.
    pub parents: f64,
    pub prefix_tokens: f64,
    pub prefix_usd: f64,
    /// The parents that spent most on agents.
    pub samples: Vec<SampleRef>,
}

/// The few main sessions that re-read the most cached context.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextInsight {
    pub head: f64,
    pub mains: f64,
    pub head_read: f64,
    pub total_read: f64,
    /// What the head's cache reads cost.
    pub usd: f64,
    pub samples: Vec<SampleRef>,
}

/// Web calls that fetched a url or query again.
#[derive(Debug, Clone, PartialEq)]
pub struct WebInsight {
    /// Repeats inside the run that already fetched it.
    pub in_run: f64,
    /// Urls or queries fetched more than once.
    pub repeated: f64,
    pub calls: f64,
    pub samples: Vec<SampleRef>,
}

/// What the corpus panels say is worth changing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CostInsights {
    /// Estimated USD of every session in the window.
    pub total_usd: f64,
    pub fanout: Option<FanoutInsight>,
    pub context: Option<ContextInsight>,
    pub web: Option<WebInsight>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CorpusPanels {
    pub context: Vec<ContextRow>,
    pub fanout: Vec<FanoutRow>,
    pub web: Vec<WebRepeatRow>,
    pub cost: Vec<CostRow>,
    pub insights: CostInsights,
}

impl CorpusPanels {
    pub fn write_context(&self, w: &mut Writer) {
        w.begin_arr();
        for r in &self.context {
            w.elem().begin_obj();
            w.key("key").text(&r.key);
            w.key("sid").text(&r.sid);
            w.key("agent").str(r.agent.as_str());
            w.key("project").text(&r.project);
            w.key("kind").str(r.kind.as_str());
            w.key("maxContextTokens").num(r.max_context_tokens);
            w.key("cacheReadTokens").num(r.cache_read_tokens);
            w.key("cacheCreationTokens").num(r.cache_creation_tokens);
            w.key("outputTokens").num(r.output_tokens);
            w.end_obj();
        }
        w.end_arr();
    }

    pub fn write_fanout(&self, w: &mut Writer) {
        w.begin_arr();
        for r in &self.fanout {
            w.elem().begin_obj();
            w.key("key").text(&r.key);
            w.key("project").text(&r.project);
            w.key("agents").num(r.agents);
            w.key("prefixTokens").num(r.prefix_tokens);
            w.key("toolCalls").num(r.tool_calls);
            w.key("totalSec").num(r.total_sec);
            w.end_obj();
        }
        w.end_arr();
    }

    pub fn write_web(&self, w: &mut Writer) {
        w.begin_arr();
        for r in &self.web {
            w.elem().begin_obj();
            w.key("key").text(&r.key);
            w.key("tool").text(&r.tool);
            w.key("calls").num(r.calls);
            w.key("sessions").num(r.sessions);
            w.key("chars").num(r.chars);
            w.key("sameRun").bool(r.same_run);
            w.end_obj();
        }
        w.end_arr();
    }

    pub fn write_cost(&self, w: &mut Writer) {
        w.begin_arr();
        for r in &self.cost {
            w.elem().begin_obj();
            w.key("key").text(&r.key);
            w.key("label").text(&r.label);
            w.key("sessions").num(r.sessions);
            w.key("inputTokens").num(r.tokens.input);
            w.key("outputTokens").num(r.tokens.output);
            w.key("cacheReadTokens").num(r.tokens.cache_read);
            w.key("cacheCreationTokens").num(r.tokens.cache_creation);
            w.end_obj();
        }
        w.end_arr();
    }
}
