//! Corpus panels from session rows: context peaks, workflow fan-out,
//! repeated web fetches and the cost estimate.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use indexmap::IndexMap;

use crate::js::num::{js_round, to_fixed};
use crate::model::Agent;
use crate::pricing::{CardCache, CostTokens, base_cost, card_for, pricing_echo, turn_cost};
use crate::stats::accumulate::FinishCtx;
use crate::stats::rank::{cmp_str, desc, rank, round, safe_div, top_key, uniq_sorted};
use crate::stats::schema::{Fact, Kind, SessionFact, UsageFact};

use super::usage::dedup_key_of;

pub use super::corpus_rows::{
    ContextInsight, ContextRow, CorpusPanels, CostInsights, CostRow, FanoutInsight, FanoutRow,
    Tokens, WebInsight, WebRepeatRow,
};

#[path = "corpus_insights.rs"]
mod insights;

const UNKNOWN_MODEL: &str = "(unknown)";
pub const WEB_TOOLS: [&str; 4] = ["WebFetch", "WebSearch", "web_search", "web_search_call"];

#[derive(Debug, Clone)]
struct SessionAgg {
    sid: String,
    agent: Agent,
    project: String,
    kind: Kind,
    parent: Option<String>,
    run: Option<String>,
    models: Vec<String>,
    max_context_tokens: f64,
    first_turn_cache_creation: f64,
    tokens: Tokens,
}

fn tokens_of(f: &SessionFact) -> Tokens {
    Tokens {
        input: f.input_tokens,
        output: f.output_tokens,
        cache_read: f.cache_read_tokens,
        cache_creation: f.cache_creation_tokens,
    }
}

#[expect(clippy::cast_precision_loss)]
fn model_slices(s: &SessionAgg) -> Vec<(String, Tokens)> {
    let models = if s.models.is_empty() {
        vec![UNKNOWN_MODEL.to_owned()]
    } else {
        uniq_sorted(s.models.iter().map(String::as_str))
    };
    let slice = 1.0 / models.len() as f64;
    models
        .into_iter()
        .map(|m| (m, s.tokens.map(|n| n * slice)))
        .collect()
}

fn session_cost(s: &SessionAgg) -> f64 {
    model_slices(s)
        .iter()
        .fold(0.0, |acc, (m, t)| acc + t.cost(m))
}

/// Per workflow run: prefix tokens, sessions per project, agent ids.
type RunAgg = (f64, IndexMap<String, f64>, HashSet<String>);

#[derive(Default)]
struct WebAgg {
    calls: f64,
    chars: f64,
    runs: HashSet<String>,
    sessions: HashSet<String>,
    /// Root session to its agent and project, for links.
    roots: IndexMap<String, (Agent, String)>,
    tool: String,
}

/// Per-call cost of one model, from usage facts.
#[derive(Default)]
struct ModelUsage {
    tokens: Tokens,
    usd: f64,
    sids: HashSet<String>,
}

#[derive(Default, Clone, Copy)]
struct CostAgg {
    sessions: f64,
    tokens: Tokens,
}

struct CostBlock<'a> {
    aggs: &'a IndexMap<String, CostAgg>,
    limit: f64,
    prefix: &'a str,
}

fn cost_block(
    b: &CostBlock<'_>,
    name: &dyn Fn(&str) -> String,
    usd_of: &dyn Fn(&str, &CostAgg) -> f64,
) -> Vec<CostRow> {
    let builds: Vec<(String, f64, CostRow)> = b
        .aggs
        .iter()
        .map(|(key, agg)| {
            let est = usd_of(key, agg);
            let k = format!("{}:{key}", b.prefix);
            let row = CostRow {
                key: k.clone(),
                label: format!("{} (~${} est)", name(key), to_fixed(est, 2)),
                sessions: agg.sessions,
                tokens: agg.tokens.map(js_round),
            };
            (k, est, row)
        })
        .collect();
    let ranked = rank(
        builds,
        |x, y| desc(x.1, y.1).then_with(|| cmp_str(&x.0, &y.0)),
        b.limit,
    );
    ranked.rows.into_iter().map(|(_, _, r)| r).collect()
}

/// The corpus panels, fed one fact at a time.
#[derive(Default)]
pub struct CorpusPanelSink {
    sessions: IndexMap<String, SessionAgg>,
    run_calls: IndexMap<String, (f64, f64)>,
    web: IndexMap<String, WebAgg>,
    web_calls: f64,
    /// Per-call cost by session, from usage facts.
    /// Every session with a usage fact has an entry, even when dedup left
    /// it no call.
    usd_by_sid: HashMap<String, f64>,
    usage_by_model: IndexMap<String, ModelUsage>,
    /// Per-call cost of cache reads alone, by session.
    read_usd_by_sid: HashMap<String, f64>,
    /// Cache-read tokens of the priced calls, by session, deduped like the
    /// dollars above.
    read_tokens_by_sid: HashMap<String, f64>,
    /// Hashes of the dedup keys priced so far.
    seen_calls: HashSet<u64>,
    cards: CardCache,
}

impl CorpusPanelSink {
    pub fn add(&mut self, fact: &Fact) {
        match fact {
            Fact::Session(f) => self.add_session(f),
            Fact::Call(c) => {
                if let Some(run) = &c.base.run {
                    let e = self.run_calls.entry(run.clone()).or_default();
                    e.0 += 1.0;
                    e.1 += c.dur_sec.unwrap_or(0.0);
                }
                if c.bash.is_none()
                    && WEB_TOOLS.contains(&c.tool.as_str())
                    && let Some(arg) = c.arg.as_deref().filter(|a| !a.is_empty())
                {
                    {
                        self.web_calls += 1.0;
                        let agg =
                            self.web
                                .entry(format!("{} {arg}", c.tool))
                                .or_insert_with(|| WebAgg {
                                    tool: c.tool.clone(),
                                    ..WebAgg::default()
                                });
                        agg.calls += 1.0;
                        agg.chars += c.out_chars;
                        agg.sessions.insert(c.base.root_sid().to_owned());
                        agg.roots
                            .entry(c.base.root_sid().to_owned())
                            .or_insert_with(|| (c.base.agent, c.base.project.clone()));
                        agg.runs.insert(c.base.run.clone().unwrap_or_default());
                    }
                }
            }
            Fact::Usage(u) => self.add_usage(u),
        }
    }

    /// Price one model call, once per request id.
    fn add_usage(&mut self, u: &UsageFact) {
        let t = Tokens {
            input: u.input_tokens,
            output: u.output_tokens,
            cache_read: u.cache_read_tokens,
            cache_creation: u.cache_creation_tokens,
        };
        if !self.sessions.contains_key(&u.sid) {
            return;
        }
        self.usd_by_sid.entry(u.sid.clone()).or_default();
        let mut h = DefaultHasher::new();
        dedup_key_of(u.agent, &u.sid, &u.req).hash(&mut h);
        if t.input + t.output + t.cache_read + t.cache_creation == 0.0
            || !self.seen_calls.insert(h.finish())
        {
            return;
        }
        let card = self.cards.get(&u.model);
        let full = CostTokens {
            input: t.input,
            output: t.output,
            cache_read: t.cache_read,
            cache_creation: t.cache_creation,
            cache_creation_1h: None,
        };
        let usd = turn_cost(card.as_ref(), &full).unwrap_or(0.0);
        *self.usd_by_sid.entry(u.sid.clone()).or_default() += usd;
        // The cache reads' share of the call at base rates, applied to the
        // call's own price, so a long prompt keeps its long-context tier.
        let reads = CostTokens {
            input: 0.0,
            output: 0.0,
            cache_creation: 0.0,
            ..full
        };
        let read_share = safe_div(
            base_cost(card.as_ref(), &reads),
            base_cost(card.as_ref(), &full),
        );
        *self.read_usd_by_sid.entry(u.sid.clone()).or_default() += usd * read_share;
        *self.read_tokens_by_sid.entry(u.sid.clone()).or_default() += t.cache_read;
        let model = if u.model.is_empty() {
            UNKNOWN_MODEL
        } else {
            &u.model
        };
        let m = self.usage_by_model.entry(model.to_owned()).or_default();
        m.tokens = m.tokens.add(t);
        m.usd += usd;
        m.sids.insert(u.sid.clone());
    }

    /// A session's estimate: its priced calls, else its totals split evenly
    /// across its models.
    fn usd_of(&self, s: &SessionAgg) -> f64 {
        self.usd_by_sid
            .get(&s.sid)
            .copied()
            .unwrap_or_else(|| session_cost(s))
    }

    fn add_session(&mut self, f: &SessionFact) {
        match self.sessions.get_mut(&f.base.sid) {
            Some(prev) => {
                if prev.run.is_none() {
                    prev.run.clone_from(&f.base.run);
                }
                let all: Vec<&str> = prev
                    .models
                    .iter()
                    .chain(&f.models)
                    .map(String::as_str)
                    .collect();
                prev.models = uniq_sorted(all);
                prev.max_context_tokens = prev.max_context_tokens.max(f.max_context_tokens);
                prev.first_turn_cache_creation += f.first_turn_cache_creation;
                prev.tokens = prev.tokens.add(tokens_of(f));
            }
            None => {
                self.sessions.insert(
                    f.base.sid.clone(),
                    SessionAgg {
                        sid: f.base.sid.clone(),
                        agent: f.base.agent,
                        project: f.base.project.clone(),
                        kind: f.base.kind,
                        parent: f.base.parent.clone(),
                        run: f.base.run.clone(),
                        models: f.models.clone(),
                        max_context_tokens: f.max_context_tokens,
                        first_turn_cache_creation: f.first_turn_cache_creation,
                        tokens: tokens_of(f),
                    },
                );
            }
        }
    }

    fn context_panel(&self, ctx: &FinishCtx<'_>) -> Vec<ContextRow> {
        let mains: Vec<&SessionAgg> = self
            .sessions
            .values()
            .filter(|s| s.kind == Kind::Main)
            .collect();
        rank(
            mains,
            |a, b| {
                desc(a.max_context_tokens, b.max_context_tokens)
                    .then_with(|| cmp_str(&a.sid, &b.sid))
            },
            ctx.limit,
        )
        .rows
        .into_iter()
        .map(|s| ContextRow {
            key: s.sid.clone(),
            sid: s.sid.clone(),
            agent: s.agent,
            project: ctx.redact.text(&s.project),
            kind: s.kind,
            max_context_tokens: s.max_context_tokens,
            cache_read_tokens: s.tokens.cache_read,
            cache_creation_tokens: s.tokens.cache_creation,
            output_tokens: s.tokens.output,
        })
        .collect()
    }

    fn workflow_agents(&self) -> Vec<&SessionAgg> {
        self.sessions
            .values()
            .filter(|s| {
                s.kind == Kind::WorkflowAgent && s.run.as_deref().is_some_and(|r| !r.is_empty())
            })
            .collect()
    }

    #[expect(clippy::cast_precision_loss)]
    fn fanout_panel(&self, ctx: &FinishCtx<'_>) -> Vec<FanoutRow> {
        let mut by_run: IndexMap<String, RunAgg> = IndexMap::new();
        for s in self.workflow_agents() {
            let e = by_run.entry(s.run.clone().unwrap_or_default()).or_default();
            e.0 += s.first_turn_cache_creation;
            *e.1.entry(s.project.clone()).or_default() += 1.0;
            e.2.insert(s.sid.clone());
        }
        let rows: Vec<FanoutRow> = by_run
            .iter()
            .map(|(run, (pre, projects, sids))| {
                let calls = self.run_calls.get(run).copied().unwrap_or_default();
                FanoutRow {
                    key: run.clone(),
                    project: ctx.redact.text(&top_key(projects)),
                    agents: sids.len() as f64,
                    prefix_tokens: *pre,
                    tool_calls: calls.0,
                    total_sec: round(calls.1),
                }
            })
            .collect();
        rank(
            rows,
            |a, b| desc(a.prefix_tokens, b.prefix_tokens).then_with(|| cmp_str(&a.key, &b.key)),
            ctx.limit,
        )
        .rows
    }

    #[expect(clippy::cast_precision_loss)]
    fn web_panel(&self, ctx: &FinishCtx<'_>) -> Vec<WebRepeatRow> {
        let repeats: Vec<WebRepeatRow> = self
            .web
            .iter()
            .filter(|(_, a)| a.calls > 1.0)
            .map(|(key, a)| WebRepeatRow {
                key: ctx.redact.line(key),
                tool: a.tool.clone(),
                calls: a.calls,
                sessions: a.sessions.len() as f64,
                chars: a.chars,
                same_run: a.runs.len() == 1 && !a.runs.contains(""),
            })
            .collect();
        rank(
            repeats,
            |a, b| desc(a.calls, b.calls).then_with(|| cmp_str(&a.key, &b.key)),
            ctx.limit,
        )
        .rows
    }

    #[expect(clippy::cast_precision_loss)]
    fn cost_panel(&self, ctx: &mut FinishCtx<'_>) -> Vec<CostRow> {
        let mut by_kind: IndexMap<String, CostAgg> = IndexMap::new();
        let mut by_project: IndexMap<String, CostAgg> = IndexMap::new();
        let mut by_model: IndexMap<String, CostAgg> = IndexMap::new();
        let mut usd_by_kind: IndexMap<String, f64> = IndexMap::new();
        let mut usd_by_project: IndexMap<String, f64> = IndexMap::new();
        let mut usd_by_model: IndexMap<String, f64> = IndexMap::new();
        let mut split = 0usize;
        let fold = |map: &mut IndexMap<String, CostAgg>, key: &str, t: Tokens| {
            let e = map.entry(key.to_owned()).or_default();
            e.sessions += 1.0;
            e.tokens = e.tokens.add(t);
        };
        for (model, u) in &self.usage_by_model {
            let e = by_model.entry(model.clone()).or_default();
            e.sessions += u.sids.len() as f64;
            e.tokens = e.tokens.add(u.tokens);
            *usd_by_model.entry(model.clone()).or_default() += u.usd;
        }
        for s in self.sessions.values() {
            fold(&mut by_kind, s.kind.as_str(), s.tokens);
            fold(&mut by_project, &s.project, s.tokens);
            let cost = self.usd_of(s);
            *usd_by_kind.entry(s.kind.as_str().to_owned()).or_default() += cost;
            *usd_by_project.entry(s.project.clone()).or_default() += cost;
            if self.usd_by_sid.contains_key(&s.sid) {
                continue;
            }
            let slices = model_slices(s);
            if slices.len() > 1 {
                split += 1;
            }
            for (model, t) in slices {
                *usd_by_model.entry(model.clone()).or_default() += t.cost(&model);
                fold(&mut by_model, &model, t);
            }
        }
        let unpriced: Vec<&str> = by_model
            .keys()
            .filter(|m| card_for(m).is_none())
            .map(String::as_str)
            .collect();
        ctx.caveat(format!(
            "Cost is estimated at API list prices from each model call's token counts. Rates per million tokens (input / output / cache write / cache read, USD): {}",
            pricing_echo(by_model.keys().map(String::as_str))
        ));
        if !unpriced.is_empty() {
            ctx.caveat(format!(
                "{} models have no price, so their calls are left out of the cost: {}",
                unpriced.len(),
                uniq_sorted(unpriced).join(", ")
            ));
        }
        if split > 0 {
            ctx.caveat(format!(
                "{split} sessions record no per-call token counts and used more than one model: their cost splits the session total evenly across those models"
            ));
        }
        let redact = ctx.redact;
        let mut out = cost_block(
            &CostBlock {
                aggs: &by_kind,
                limit: by_kind.len() as f64,
                prefix: "kind",
            },
            &|k| k.to_owned(),
            &|k, _| usd_by_kind.get(k).copied().unwrap_or(0.0),
        );
        let limit = ctx.limit;
        out.extend(cost_block(
            &CostBlock {
                aggs: &by_model,
                limit,
                prefix: "model",
            },
            &|k| {
                if card_for(k).is_some() {
                    k.to_owned()
                } else {
                    format!("{k} (unpriced)")
                }
            },
            &|k, _| usd_by_model.get(k).copied().unwrap_or(0.0),
        ));
        out.extend(cost_block(
            &CostBlock {
                aggs: &by_project,
                limit,
                prefix: "project",
            },
            &|k| redact.text(k),
            &|k, _| usd_by_project.get(k).copied().unwrap_or(0.0),
        ));
        out
    }

    pub fn finish(&self, ctx: &mut FinishCtx<'_>) -> CorpusPanels {
        CorpusPanels {
            context: self.context_panel(ctx),
            fanout: self.fanout_panel(ctx),
            web: self.web_panel(ctx),
            cost: self.cost_panel(ctx),
            insights: self.insights(ctx),
        }
    }
}
