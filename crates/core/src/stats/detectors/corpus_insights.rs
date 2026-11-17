//! The corpus panels' cost insights: agent fan-out, context concentration and
//! repeated web fetches, each with the main sessions behind it.

use indexmap::IndexMap;

use super::{CorpusPanelSink, SessionAgg, Tokens, model_slices};
use crate::js::num::to_js_string;
use crate::model::Agent;
use crate::stats::accumulate::{FinishCtx, SampleRef};
use crate::stats::detectors::corpus_rows::{
    ContextInsight, CostInsights, FanoutInsight, WebInsight,
};
use crate::stats::rank::{cmp_str, desc};
use crate::stats::schema::Kind;

/// Share of main sessions the context insight calls "a few".
const CONCENTRATION_HEAD: f64 = 0.01;

/// The cost of only some of a session's tokens, split like its totals.
fn part_cost(s: &SessionAgg, pick: impl Fn(Tokens) -> Tokens) -> f64 {
    model_slices(s)
        .iter()
        .fold(0.0, |acc, (m, t)| acc + pick(*t).cost(m))
}

/// A link to a main session, with one line on why it is listed.
fn main_ref(
    ctx: &FinishCtx<'_>,
    sid: &str,
    agent: Agent,
    project: &str,
    line: String,
) -> SampleRef {
    SampleRef {
        sid: sid.to_owned(),
        agent,
        project: ctx.redact.text(project),
        kind: Kind::Main,
        parent: None,
        seq: 0.0,
        tool_use_id: None,
        ts: None,
        line,
    }
}

fn dollars(usd: f64) -> String {
    format!("~${}", to_js_string(usd.round()))
}

/// Per parent session: agent count, agent cost, agent and project.
type ParentAgg = (f64, f64, Agent, String);

impl CorpusPanelSink {
    #[expect(clippy::cast_precision_loss)]
    fn fanout_insight(&self, ctx: &FinishCtx<'_>) -> Option<FanoutInsight> {
        let nested: Vec<&SessionAgg> = self
            .sessions
            .values()
            .filter(|s| s.kind != Kind::Main)
            .collect();
        if nested.is_empty() {
            return None;
        }
        let mut parents: IndexMap<String, ParentAgg> = IndexMap::new();
        for s in &nested {
            let Some(root) = s.parent.clone() else {
                continue;
            };
            let e = parents
                .entry(root)
                .or_insert_with(|| (0.0, 0.0, s.agent, s.project.clone()));
            e.0 += 1.0;
            e.1 += self.usd_of(s);
        }
        let mut ranked: Vec<(&String, &ParentAgg)> = parents.iter().collect();
        ranked.sort_by(|a, b| desc(a.1.1, b.1.1).then_with(|| cmp_str(a.0, b.0)));
        let samples = ranked
            .iter()
            .take(ctx.sample_cap)
            .map(|(sid, (agents, usd, agent, project))| {
                let unit = if *agents == 1.0 { "agent" } else { "agents" };
                let line = format!("{} {unit}, {}", to_js_string(*agents), dollars(*usd));
                main_ref(ctx, sid, *agent, project, line)
            })
            .collect();
        let first_turn = |t: Tokens| Tokens {
            cache_creation: t.cache_creation,
            ..Tokens::default()
        };
        let workflow = self.workflow_agents();
        Some(FanoutInsight {
            usd: nested.iter().map(|s| self.usd_of(s)).sum(),
            agents: nested.len() as f64,
            parents: parents.len() as f64,
            prefix_tokens: workflow.iter().map(|s| s.first_turn_cache_creation).sum(),
            prefix_usd: workflow
                .iter()
                .map(|s| {
                    let share = if s.tokens.cache_creation > 0.0 {
                        s.first_turn_cache_creation / s.tokens.cache_creation
                    } else {
                        0.0
                    };
                    part_cost(s, first_turn) * share
                })
                .sum(),
            samples,
        })
    }

    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn context_insight(&self, ctx: &FinishCtx<'_>) -> Option<ContextInsight> {
        let mut mains: Vec<&SessionAgg> = self
            .sessions
            .values()
            .filter(|s| s.kind == Kind::Main)
            .collect();
        let total_read: f64 = mains.iter().map(|s| s.tokens.cache_read).sum();
        if total_read <= 0.0 {
            return None;
        }
        mains.sort_by(|a, b| {
            desc(a.tokens.cache_read, b.tokens.cache_read).then_with(|| cmp_str(&a.sid, &b.sid))
        });
        let head = ((mains.len() as f64 * CONCENTRATION_HEAD).ceil() as usize).max(1);
        let top = &mains[..head.min(mains.len())];
        let read_only = |t: Tokens| Tokens {
            cache_read: t.cache_read,
            ..Tokens::default()
        };
        Some(ContextInsight {
            head: top.len() as f64,
            mains: mains.len() as f64,
            head_read: top.iter().map(|s| s.tokens.cache_read).sum(),
            total_read,
            usd: top
                .iter()
                .map(|s| {
                    if self.usd_by_sid.contains_key(&s.sid) {
                        self.read_usd_by_sid.get(&s.sid).copied().unwrap_or(0.0)
                    } else {
                        part_cost(s, read_only)
                    }
                })
                .sum(),
            samples: top
                .iter()
                .take(ctx.sample_cap)
                .map(|s| {
                    let line = format!(
                        "{} tokens re-read from cache",
                        crate::stats::format::tokens(s.tokens.cache_read)
                    );
                    main_ref(ctx, &s.sid, s.agent, &s.project, line)
                })
                .collect(),
        })
    }

    #[expect(clippy::cast_precision_loss)]
    fn web_insight(&self, ctx: &FinishCtx<'_>) -> Option<WebInsight> {
        let mut repeats: Vec<(&String, &super::WebAgg)> =
            self.web.iter().filter(|(_, a)| a.calls > 1.0).collect();
        if repeats.is_empty() {
            return None;
        }
        let in_run = repeats
            .iter()
            .filter(|(_, a)| a.runs.len() == 1 && !a.runs.contains(""))
            .map(|(_, a)| a.calls - 1.0)
            .sum();
        repeats.sort_by(|a, b| desc(a.1.calls, b.1.calls).then_with(|| cmp_str(a.0, b.0)));
        let mut roots: IndexMap<&str, (Agent, &str, f64)> = IndexMap::new();
        for (_, a) in &repeats {
            for (sid, (agent, project)) in &a.roots {
                roots.entry(sid).or_insert((*agent, project, 0.0)).2 += a.calls;
            }
        }
        let mut roots: Vec<_> = roots.into_iter().collect();
        roots.sort_by(|a, b| desc(a.1.2, b.1.2).then_with(|| cmp_str(a.0, b.0)));
        Some(WebInsight {
            in_run,
            repeated: repeats.len() as f64,
            calls: self.web_calls,
            samples: roots
                .iter()
                .take(ctx.sample_cap)
                .map(|(sid, (agent, project, _))| {
                    main_ref(ctx, sid, *agent, project, "fetched a url again".to_owned())
                })
                .collect(),
        })
    }

    /// USD per cache-read token of one session, at its own model's price;
    /// `None` when the session has no model call to price.
    pub fn read_rate(&self, sid: &str) -> Option<f64> {
        let s = self.sessions.get(sid)?;
        let (usd, read) = if self.usd_by_sid.contains_key(sid) {
            (
                self.read_usd_by_sid.get(sid).copied().unwrap_or(0.0),
                self.read_tokens_by_sid.get(sid).copied().unwrap_or(0.0),
            )
        } else {
            let usd = part_cost(s, |t| Tokens {
                cache_read: t.cache_read,
                ..Tokens::default()
            });
            (usd, s.tokens.cache_read)
        };
        (usd > 0.0 && read > 0.0).then(|| usd / read)
    }

    /// USD per cache-read token over every priced call, for a session with
    /// no price of its own.
    pub fn mean_read_rate(&self) -> Option<f64> {
        let usd: f64 = self.read_usd_by_sid.values().sum();
        let read: f64 = self.read_tokens_by_sid.values().sum();
        (usd > 0.0 && read > 0.0).then(|| usd / read)
    }

    pub(super) fn insights(&self, ctx: &FinishCtx<'_>) -> CostInsights {
        CostInsights {
            total_usd: self.sessions.values().map(|s| self.usd_of(s)).sum(),
            fanout: self.fanout_insight(ctx),
            context: self.context_insight(ctx),
            web: self.web_insight(ctx),
        }
    }
}
