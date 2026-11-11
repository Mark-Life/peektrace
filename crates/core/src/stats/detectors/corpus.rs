//! Corpus panels from session rows: context peaks, workflow fan-out,
//! repeated web fetches and the cost estimate.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::js::num::{js_round, to_fixed, to_js_string};
use crate::model::Agent;
use crate::pricing::{card_for, pricing_echo};
use crate::stats::accumulate::FinishCtx;
use crate::stats::rank::{cmp_str, desc, rank, round, share, top_key, uniq_sorted};
use crate::stats::schema::{Fact, Kind, SessionFact};

pub use super::corpus_rows::{ContextRow, CorpusPanels, CostRow, FanoutRow, Tokens, WebRepeatRow};

const UNKNOWN_MODEL: &str = "(unknown)";
const CONCENTRATION_HEAD: f64 = 0.01;
pub const WEB_TOOLS: [&str; 4] = ["WebFetch", "WebSearch", "web_search", "web_search_call"];

#[derive(Debug, Clone)]
struct SessionAgg {
    sid: String,
    agent: Agent,
    project: String,
    kind: Kind,
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

/// Token counts short enough to read inside a sentence.
pub fn compact(n: f64) -> String {
    let abs = n.abs();
    if abs >= 1e9 {
        format!("{}B", to_fixed(n / 1e9, 2))
    } else if abs >= 1e6 {
        format!("{}M", to_fixed(n / 1e6, 2))
    } else if abs >= 1e3 {
        format!("{}k", to_fixed(n / 1e3, 1))
    } else {
        to_js_string(js_round(n))
    }
}

fn pct(part: f64, total: f64) -> String {
    format!("{}%", to_fixed(share(part, total) * 100.0, 1))
}

/// Per workflow run: prefix tokens, sessions per project, agent ids.
type RunAgg = (f64, IndexMap<String, f64>, HashSet<String>);

#[derive(Default)]
struct WebAgg {
    calls: f64,
    chars: f64,
    runs: HashSet<String>,
    sessions: HashSet<String>,
    tool: String,
}

#[derive(Default, Clone, Copy)]
struct CostAgg {
    sessions: f64,
    tokens: Tokens,
}

fn note_dropped(ctx: &mut FinishCtx<'_>, dropped: usize, what: &str) {
    if dropped > 0 {
        ctx.caveat(format!("{dropped} more {what} not shown"));
    }
}

struct CostBlock<'a> {
    aggs: &'a IndexMap<String, CostAgg>,
    limit: f64,
    prefix: &'a str,
    what: &'a str,
}

fn cost_block(
    ctx: &mut FinishCtx<'_>,
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
    note_dropped(ctx, ranked.dropped, b.what);
    ranked.rows.into_iter().map(|(_, _, r)| r).collect()
}

/// The corpus panels, fed one fact at a time.
#[derive(Default)]
pub struct CorpusPanelSink {
    sessions: IndexMap<String, SessionAgg>,
    run_calls: IndexMap<String, (f64, f64)>,
    web: IndexMap<String, WebAgg>,
    web_calls: f64,
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
                        agg.sessions.insert(c.base.sid.clone());
                        agg.runs.insert(c.base.run.clone().unwrap_or_default());
                    }
                }
            }
            Fact::Usage(_) => {}
        }
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

    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn context_panel(&self, ctx: &mut FinishCtx<'_>) -> Vec<ContextRow> {
        let mains: Vec<&SessionAgg> = self
            .sessions
            .values()
            .filter(|s| s.kind == Kind::Main)
            .collect();
        let total_read = mains.iter().fold(0.0, |a, s| a + s.tokens.cache_read);
        let head = ((mains.len() as f64 * CONCENTRATION_HEAD).ceil() as usize).max(1);
        let mut by_read = mains.clone();
        by_read.sort_by(|a, b| {
            desc(a.tokens.cache_read, b.tokens.cache_read).then_with(|| cmp_str(&a.sid, &b.sid))
        });
        let head_read = by_read
            .iter()
            .take(head)
            .fold(0.0, |a, s| a + s.tokens.cache_read);
        if total_read > 0.0 {
            ctx.caveat(format!(
                "Context: the top 1% of main sessions ({head} of {}) hold {} of {} cache-read tokens — compact or split those sessions before tuning anything else",
                mains.len(),
                pct(head_read, total_read),
                compact(total_read)
            ));
        }
        let ranked = rank(
            mains,
            |a, b| {
                desc(a.max_context_tokens, b.max_context_tokens)
                    .then_with(|| cmp_str(&a.sid, &b.sid))
            },
            ctx.limit,
        );
        note_dropped(ctx, ranked.dropped, "context sessions");
        ranked
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

    #[expect(clippy::cast_precision_loss)]
    fn fanout_panel(&self, ctx: &mut FinishCtx<'_>) -> Vec<FanoutRow> {
        let agents: Vec<&SessionAgg> = self
            .sessions
            .values()
            .filter(|s| {
                s.kind == Kind::WorkflowAgent && s.run.as_deref().is_some_and(|r| !r.is_empty())
            })
            .collect();
        let mut by_run: IndexMap<String, RunAgg> = IndexMap::new();
        for s in &agents {
            let e = by_run.entry(s.run.clone().unwrap_or_default()).or_default();
            e.0 += s.first_turn_cache_creation;
            *e.1.entry(s.project.clone()).or_default() += 1.0;
            e.2.insert(s.sid.clone());
        }
        let created = agents.iter().fold(0.0, |a, s| a + s.tokens.cache_creation);
        let prefix = agents
            .iter()
            .fold(0.0, |a, s| a + s.first_turn_cache_creation);
        if created > 0.0 {
            ctx.caveat(format!(
                "Fan-out: {} of workflow cache creation ({} of {} tokens) is first-turn prefix, re-paid by every agent a run spawns — shrink the shared prefix or spawn fewer agents; filtered to {} workflow-agent transcripts across {} runs",
                pct(prefix, created),
                compact(prefix),
                compact(created),
                agents.len(),
                by_run.len()
            ));
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
        let ranked = rank(
            rows,
            |a, b| desc(a.prefix_tokens, b.prefix_tokens).then_with(|| cmp_str(&a.key, &b.key)),
            ctx.limit,
        );
        note_dropped(ctx, ranked.dropped, "workflow runs");
        ranked.rows
    }

    #[expect(clippy::cast_precision_loss)]
    fn web_panel(&self, ctx: &mut FinishCtx<'_>) -> Vec<WebRepeatRow> {
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
        let in_run = repeats
            .iter()
            .filter(|r| r.same_run)
            .fold(0.0, |a, r| a + (r.calls - 1.0));
        if !repeats.is_empty() {
            ctx.caveat(format!(
                "Web: {} of {} web calls re-fetched a url or query already answered inside the same run — cache the fetch at run level; {} urls were fetched more than once overall",
                to_js_string(in_run),
                to_js_string(self.web_calls),
                repeats.len()
            ));
        }
        let ranked = rank(
            repeats,
            |a, b| desc(a.calls, b.calls).then_with(|| cmp_str(&a.key, &b.key)),
            ctx.limit,
        );
        note_dropped(ctx, ranked.dropped, "repeated web fetches");
        ranked.rows
    }

    #[expect(clippy::cast_precision_loss)]
    fn cost_panel(&self, ctx: &mut FinishCtx<'_>) -> Vec<CostRow> {
        let mut by_kind: IndexMap<String, CostAgg> = IndexMap::new();
        let mut by_project: IndexMap<String, CostAgg> = IndexMap::new();
        let mut by_model: IndexMap<String, CostAgg> = IndexMap::new();
        let mut usd_by_kind: IndexMap<String, f64> = IndexMap::new();
        let mut usd_by_project: IndexMap<String, f64> = IndexMap::new();
        let mut unpriced: Vec<String> = Vec::new();
        let mut multi_model = 0usize;
        let fold = |map: &mut IndexMap<String, CostAgg>, key: &str, t: Tokens| {
            let e = map.entry(key.to_owned()).or_default();
            e.sessions += 1.0;
            e.tokens = e.tokens.add(t);
        };
        for s in self.sessions.values() {
            fold(&mut by_kind, s.kind.as_str(), s.tokens);
            fold(&mut by_project, &s.project, s.tokens);
            let cost = session_cost(s);
            *usd_by_kind.entry(s.kind.as_str().to_owned()).or_default() += cost;
            *usd_by_project.entry(s.project.clone()).or_default() += cost;
            let slices = model_slices(s);
            if slices.len() > 1 {
                multi_model += 1;
            }
            for (model, t) in slices {
                fold(&mut by_model, &model, t);
                if card_for(&model).is_none() && !unpriced.contains(&model) {
                    unpriced.push(model);
                }
            }
        }
        ctx.caveat(format!(
            "Cost is an ESTIMATE, not a measurement: no transcript records per-call tokens. Rates per million tokens (input/output/cacheWrite/cacheRead USD): {}",
            pricing_echo(by_model.keys().map(String::as_str))
        ));
        if !unpriced.is_empty() {
            ctx.caveat(format!(
                "Cost: {} model ids have no rate card and are left out of the cost — {}",
                unpriced.len(),
                uniq_sorted(unpriced.iter().map(String::as_str)).join(", ")
            ));
        }
        if multi_model > 0 {
            ctx.caveat(format!(
                "Cost by model splits each session's tokens evenly across the models it used, because a session fact records no per-model split; {multi_model} of {} sessions used more than one model",
                self.sessions.len()
            ));
        }
        let redact = ctx.redact;
        let mut out = cost_block(
            ctx,
            &CostBlock {
                aggs: &by_kind,
                limit: by_kind.len() as f64,
                prefix: "kind",
                what: "session kinds",
            },
            &|k| k.to_owned(),
            &|k, _| usd_by_kind.get(k).copied().unwrap_or(0.0),
        );
        let limit = ctx.limit;
        out.extend(cost_block(
            ctx,
            &CostBlock {
                aggs: &by_model,
                limit,
                prefix: "model",
                what: "models",
            },
            &|k| {
                if card_for(k).is_some() {
                    k.to_owned()
                } else {
                    format!("{k} (unpriced)")
                }
            },
            &|k, a| a.tokens.cost(k),
        ));
        out.extend(cost_block(
            ctx,
            &CostBlock {
                aggs: &by_project,
                limit,
                prefix: "project",
                what: "projects",
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
        }
    }
}
