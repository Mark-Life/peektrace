//! Context forensics for a parsed session: budget attribution per turn, the
//! peak, the dumb zone, and cost. Retained thinking is stored as empty text,
//! so its size comes from `thinking_tokens`, else output minus visible text.

use std::collections::HashMap;

use super::context_edits::{RequestSize, request_sizes, size_steps, tokens_at, tokens_before};
use super::cost::{price_turns, session_cost};
use super::model::{
    AnalyzedSession, BudgetKey, BudgetSlice, BudgetSlices, EventKind, LoadedCategory,
    OnDiskContextFile, ParsedSession, TimelineEvent, Turn, TurnSnapshot,
};
use crate::js::time::{self, LocalZone};

const DEFAULT_WINDOW: f64 = 1_000_000.0;
const DEFAULT_DUMB_ZONE: f64 = 0.4;

/// Analysis inputs besides the session.
pub struct AnalyzeOptions<'a> {
    /// Degradation threshold as a fraction of the window.
    pub dumb_zone_fraction: Option<f64>,
    pub on_disk_context_files: Vec<OnDiskContextFile>,
    /// Context-window override (tokens).
    pub window: Option<f64>,
    /// Zone for timestamps without an offset.
    pub local: &'a dyn LocalZone,
}

struct CatMeta {
    label: &'static str,
    short: &'static str,
    color: &'static str,
    note: Option<&'static str>,
}

const fn cat_meta(key: BudgetKey) -> CatMeta {
    let (label, short, color, note) = match key {
        BudgetKey::SystemTools => (
            "System + tool definitions",
            "System",
            "#6e7681",
            Some("inferred floor — not in transcript"),
        ),
        BudgetKey::Listings => ("Skill / agent / tool listings", "Listings", "#58a6ff", None),
        BudgetKey::Memory => (
            "CLAUDE.md / AGENTS.md (in transcript)",
            "Memory",
            "#bc8cff",
            None,
        ),
        BudgetKey::Files => ("Opened files / plans", "Files", "#39c5cf", None),
        BudgetKey::Prompts => ("User prompts", "Prompts", "#3fb950", None),
        BudgetKey::ToolResults => ("Tool results", "Tool results", "#f0883e", None),
        BudgetKey::AssistantText => ("Assistant text + tool calls", "Assistant", "#d29922", None),
        BudgetKey::Thinking => (
            "Assistant thinking (retained)",
            "Thinking",
            "#db61a2",
            Some("usage thinking_tokens where logged, else output_tokens − visible text"),
        ),
        BudgetKey::Other => (
            "Other injected (reminders, hooks, notifications)",
            "Other",
            "#8a929e",
            None,
        ),
        BudgetKey::Unattributed => (
            "Tool schemas & overhead (not in transcript)",
            "Overhead",
            "#484f58",
            Some("real context − everything attributable above"),
        ),
    };
    CatMeta {
        label,
        short,
        color,
        note,
    }
}

/// Running per-category totals (`unattributed` and `thinking` unused).
type Cat = BudgetSlices;

/// What an event adds to: its loaded category first, else its kind.
#[derive(Debug, Clone, Copy)]
struct FoldTarget {
    category: Option<LoadedCategory>,
    kind: EventKind,
    harness_prompt: bool,
}

impl FoldTarget {
    fn of(e: &TimelineEvent) -> Self {
        Self {
            category: e.loaded_category,
            kind: e.kind,
            harness_prompt: e.kind == EventKind::UserPrompt && !e.is_user_authored(),
        }
    }

    fn fold(self, cat: &mut Cat, tokens: f64) {
        match self.category {
            Some(LoadedCategory::ClaudeMd) => return cat.memory += tokens,
            Some(
                LoadedCategory::Skills
                | LoadedCategory::Agents
                | LoadedCategory::Tools
                | LoadedCategory::Mcp,
            ) => return cat.listings += tokens,
            Some(LoadedCategory::File | LoadedCategory::Ide) => return cat.files += tokens,
            Some(LoadedCategory::Reminder | LoadedCategory::Other) => return cat.other += tokens,
            Some(LoadedCategory::Memory) | None => {}
        }
        if self.kind == EventKind::ToolResult {
            cat.tool_results += tokens;
        } else if self.harness_prompt {
            cat.other += tokens;
        } else if matches!(self.kind, EventKind::UserPrompt | EventKind::Compaction) {
            cat.prompts += tokens;
        } else if self.kind == EventKind::Summary {
            cat.other += tokens;
        }
    }
}

/// Per-turn slices that sum to `ctx` with every value >= 0.
fn make_slices(ctx: f64, cat: &Cat, retained_thinking: f64) -> BudgetSlices {
    let floor = cat.system_tools.min(ctx);
    let rest = 0.0_f64.max(ctx - floor);
    let est = [
        retained_thinking,
        cat.assistant_text,
        cat.tool_results,
        cat.prompts,
        cat.memory,
        cat.listings,
        cat.files,
        cat.other,
    ];
    let est_sum = est.iter().fold(0.0, |a, b| a + b);
    let scale = if est_sum > rest && est_sum > 0.0 {
        rest / est_sum
    } else {
        1.0
    };
    BudgetSlices {
        system_tools: floor,
        thinking: est[0] * scale,
        assistant_text: est[1] * scale,
        tool_results: est[2] * scale,
        prompts: est[3] * scale,
        memory: est[4] * scale,
        listings: est[5] * scale,
        files: est[6] * scale,
        other: est[7] * scale,
        unattributed: if scale < 1.0 {
            0.0
        } else {
            0.0_f64.max(rest - est_sum)
        },
    }
}

/// A later context edit's effect, applied once the walk passes its line.
#[derive(Debug, Clone, Copy)]
enum Pending {
    Request {
        at_index: f64,
        visible: f64,
        thinking: f64,
    },
    Fold {
        at_index: f64,
        target: FoldTarget,
        delta: f64,
    },
}

impl Pending {
    fn at_index(&self) -> f64 {
        match self {
            Self::Request { at_index, .. } | Self::Fold { at_index, .. } => *at_index,
        }
    }
}

#[derive(Default)]
struct Walk {
    cat: Cat,
    edits: Vec<Pending>,
    last_snap_req: Option<String>,
    pending_compaction: bool,
    retained_thinking: f64,
    snapshots: Vec<TurnSnapshot>,
    compaction_turns: Vec<usize>,
}

impl Walk {
    fn apply_edits(&mut self, line: f64) {
        if !self.edits.iter().any(|d| d.at_index() < line) {
            return;
        }
        let (due, rest): (Vec<_>, Vec<_>) = self.edits.iter().partition(|d| d.at_index() < line);
        self.edits = rest;
        for d in due {
            match d {
                Pending::Request {
                    visible, thinking, ..
                } => {
                    self.cat.assistant_text += visible;
                    self.retained_thinking += thinking;
                }
                Pending::Fold { target, delta, .. } => target.fold(&mut self.cat, delta),
            }
        }
    }

    fn snapshot(&mut self, turn: &Turn, ti: usize, size: Option<&RequestSize>) {
        self.snapshots.push(TurnSnapshot {
            turn_index: ti,
            model: turn.model.clone(),
            ctx: turn.context_tokens,
            output_tokens: turn.output_tokens,
            cache_read_tokens: turn.cache_read_tokens,
            slices: make_slices(turn.context_tokens, &self.cat, self.retained_thinking),
            ts: turn.ts.clone(),
        });
        if self.pending_compaction {
            self.compaction_turns.push(ti);
            self.pending_compaction = false;
        }
        self.last_snap_req = Some(turn.request_id.clone());
        let reported = turn.thinking_tokens.map(|t| t.min(turn.output_tokens));
        let tt = match reported {
            None => size.map_or(0.0, |s| s.visible),
            Some(r) => turn.output_tokens - r,
        };
        let thinking = reported.unwrap_or_else(|| 0.0_f64.max(turn.output_tokens - tt));
        self.cat.assistant_text += tt;
        self.retained_thinking += thinking;
        let mut prev = (tt, thinking);
        for step in size.map(|s| s.steps.as_slice()).unwrap_or_default() {
            self.edits.push(Pending::Request {
                at_index: step.at_index,
                visible: step.visible - prev.0,
                thinking: step.thinking - prev.1,
            });
            prev = (step.visible, step.thinking);
        }
    }

    /// Compaction rewrites history: every bucket but the system floor
    /// clears. Listings come back after the summary; memory does not.
    fn fold_non_turn(&mut self, e: &TimelineEvent) {
        if e.kind == EventKind::Compaction {
            self.pending_compaction = true;
            self.retained_thinking = 0.0;
            self.cat = Cat {
                system_tools: self.cat.system_tools,
                ..Cat::default()
            };
            self.edits.clear();
        }
        if e.kind == EventKind::SystemPrompt {
            return;
        }
        let target = FoldTarget::of(e);
        let mut prev = tokens_before(e);
        target.fold(&mut self.cat, prev);
        for step in size_steps(e) {
            self.edits.push(Pending::Fold {
                at_index: step.at_index,
                target,
                delta: step.tokens_est - prev,
            });
            prev = step.tokens_est;
        }
    }
}

#[expect(clippy::cast_precision_loss, reason = "line indexes fit f64")]
fn line_of(e: &TimelineEvent) -> f64 {
    e.index as f64
}

/// Single ordered walk: accumulate retained content, snapshot each turn.
fn walk_turns(p: &ParsedSession, system_overhead: f64) -> (Vec<TurnSnapshot>, Vec<usize>) {
    let index_by_req: HashMap<&str, usize> = p
        .turns
        .iter()
        .enumerate()
        .map(|(i, t)| (t.request_id.as_str(), i))
        .collect();
    let sizes = request_sizes(&p.events);
    let mut ws = Walk {
        cat: Cat {
            system_tools: system_overhead,
            ..Cat::default()
        },
        ..Walk::default()
    };
    for e in &p.events {
        ws.apply_edits(line_of(e));
        let req = e.request_id.as_deref().filter(|r| !r.is_empty());
        match req.and_then(|r| index_by_req.get(r).map(|&ti| (r, ti))) {
            Some((r, ti)) => {
                if ws.last_snap_req.as_deref() != Some(r) {
                    ws.snapshot(&p.turns[ti], ti, sizes.get(r));
                }
            }
            None if !e.sidechain() => ws.fold_non_turn(e),
            None => {}
        }
    }
    (ws.snapshots, ws.compaction_turns)
}

/// The system and tool floor: the first turn's context minus what the
/// transcript shows before it.
fn system_overhead(p: &ParsedSession) -> f64 {
    let first_req = p.turns.first().map(|t| t.request_id.as_str());
    let first_pos = p
        .events
        .iter()
        .position(|e| e.request_id.as_deref() == first_req)
        .unwrap_or(p.events.len());
    let first_line = p.events.get(first_pos).map_or(f64::INFINITY, line_of);
    let visible = p.events[..first_pos]
        .iter()
        .filter(|e| e.kind != EventKind::SystemPrompt)
        .fold(0.0, |n, e| n + tokens_at(e, first_line));
    0.0_f64.max(p.turns.first().map_or(0.0, |t| t.context_tokens) - visible)
}

/// Analyze a parsed session into render-ready metrics.
pub fn analyze(mut p: ParsedSession, opts: AnalyzeOptions<'_>) -> AnalyzedSession {
    let dumb_zone_fraction = opts.dumb_zone_fraction.unwrap_or(DEFAULT_DUMB_ZONE);
    let (mut peak, mut peak_index, mut peak_cache_read, mut total_output) = (0.0, 0, 0.0, 0.0);
    for (i, t) in p.turns.iter().enumerate() {
        total_output += t.output_tokens;
        if t.context_tokens > peak {
            peak = t.context_tokens;
            peak_index = i;
            peak_cache_read = t.cache_read_tokens;
        }
    }
    let final_context = p.turns.last().map_or(0.0, |t| t.context_tokens);
    let native = p.native_context_window;
    let context_window = opts.window.or(native).unwrap_or(DEFAULT_WINDOW);
    let overhead = system_overhead(&p);
    let (snapshots, compaction_turns) = walk_turns(&p, overhead);
    let peak_snap = snapshots
        .iter()
        .find(|s| s.turn_index == peak_index)
        .or_else(|| snapshots.last());
    let budget = BudgetKey::ALL
        .into_iter()
        .map(|key| {
            let m = cat_meta(key);
            BudgetSlice {
                key,
                label: m.label,
                short: m.short,
                color: m.color,
                estimated: true,
                note: m.note,
                tokens: peak_snap.map_or(0.0, |s| s.slices.get(key)),
            }
        })
        .filter(|s| s.tokens > 0.0)
        .collect();
    let dumb_zone_tokens = dumb_zone_fraction * context_window;
    let in_zone: Vec<usize> = p
        .turns
        .iter()
        .enumerate()
        .filter(|(_, t)| t.context_tokens >= dumb_zone_tokens)
        .map(|(i, _)| i)
        .collect();
    let user_message_count = p.events.iter().filter(|e| e.is_user_authored()).count();
    let tool_call_count = p
        .events
        .iter()
        .filter(|e| e.kind == EventKind::ToolCall)
        .count();
    let truthy = |s: &Option<String>| s.as_deref().filter(|s| !s.is_empty()).map(str::to_owned);
    #[expect(clippy::cast_precision_loss, reason = "dates are within 8.64e15")]
    let duration_ms = truthy(&p.started_at)
        .zip(truthy(&p.ended_at))
        .map(|(s, e)| {
            match (
                time::parse_date(&e, opts.local),
                time::parse_date(&s, opts.local),
            ) {
                (Some(e), Some(s)) => (e - s) as f64,
                _ => f64::NAN,
            }
        });
    let sum =
        |f: fn(&Turn) -> Option<f64>| p.turns.iter().fold(0.0, |n, t| n + f(t).unwrap_or(0.0));
    let web_search_requests = sum(|t| t.web_search_requests);
    let web_fetch_requests = sum(|t| t.web_fetch_requests);
    let turn_count = p.turns.len();
    price_turns(&mut p.turns);
    let cost = session_cost(&p, &p.turns);
    AnalyzedSession {
        parsed: p,
        context_window,
        context_window_inferred: opts.window.is_none() && native.is_none(),
        peak_context_tokens: peak,
        peak_turn_index: peak_index,
        final_context_tokens: final_context,
        total_output_tokens: total_output,
        system_overhead_tokens: overhead,
        budget,
        snapshots,
        on_disk_context_files: opts.on_disk_context_files,
        dumb_zone_cross_turn: in_zone
            .first()
            .map_or(-1, |&i| i64::try_from(i).unwrap_or(i64::MAX)),
        dumb_zone_fraction,
        dumb_zone_turns: in_zone.len(),
        compaction_turns,
        turn_count,
        user_message_count,
        tool_call_count,
        peak_cache_read_tokens: peak_cache_read,
        web_search_requests,
        web_fetch_requests,
        duration_ms,
        cost,
    }
}
