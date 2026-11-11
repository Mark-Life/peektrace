//! JSON for the session model, in the key order the TS core inserts keys.

mod event;

use event::usize_json;
pub use event::{write_event, write_turn};

use crate::js::json_out::Writer;
use crate::sessions::model::{
    AnalyzedSession, BudgetKey, BudgetSlices, OnDiskContextFile, ParsedSession, SessionCost,
    SubagentRef, TurnsCost,
};

fn write_cost_fields(w: &mut Writer, c: &TurnsCost) {
    if let Some(usd) = c.cost_usd {
        w.key("costUsd").num(usd);
    }
    if let Some(models) = &c.unpriced_models {
        w.key("unpricedModels")
            .texts(models.iter().map(String::as_str));
    }
}

pub fn write_subagent(w: &mut Writer, s: &SubagentRef) {
    let opt = |w: &mut Writer, key: &str, v: Option<&String>| {
        if let Some(v) = v {
            w.key(key).text(v);
        }
    };
    w.begin_obj();
    w.key("id").text(&s.id);
    w.key("path").text(&s.path);
    opt(w, "agentType", s.agent_type.as_ref());
    opt(w, "description", s.description.as_ref());
    if !s.tool_use_id_linked {
        opt(w, "toolUseId", s.tool_use_id.as_ref());
    }
    opt(w, "parentAgentId", s.parent_agent_id.as_ref());
    opt(w, "workflowRunId", s.workflow_run_id.as_ref());
    opt(w, "workflowPhase", s.workflow_phase.as_ref());
    if let Some(i) = s.workflow_phase_index {
        w.key("workflowPhaseIndex").num(i);
    }
    if s.tool_use_id_linked {
        opt(w, "toolUseId", s.tool_use_id.as_ref());
    }
    w.key("turns").int(usize_json(s.turns));
    w.key("peakContextTokens").num(s.peak_context_tokens);
    write_cost_fields(w, &s.cost);
    if s.unreadable {
        w.key("unreadable").bool(true);
    }
    w.end_obj();
}

fn write_counts(w: &mut Writer, key: &str, counts: Option<&Vec<(String, f64)>>) {
    if let Some(counts) = counts {
        w.key(key).begin_obj();
        for (k, n) in counts {
            w.key_text(k).num(*n);
        }
        w.end_obj();
    }
}

/// The parsed-session keys, without the enclosing braces.
fn write_parsed_fields(w: &mut Writer, p: &ParsedSession) {
    let opt = |w: &mut Writer, key: &str, v: Option<&String>| {
        if let Some(v) = v {
            w.key(key).text(v);
        }
    };
    w.key("provider").str(p.provider.as_str());
    w.key("sessionId").text(&p.session_id);
    w.key("path").text(&p.path);
    w.key("models").texts(p.models.iter().map(String::as_str));
    w.key("events").begin_arr();
    p.events.iter().for_each(|e| write_event(w.elem(), e));
    w.end_arr();
    w.key("turns").begin_arr();
    p.turns.iter().for_each(|t| write_turn(w.elem(), t));
    w.end_arr();
    w.key("compactionIndexes").begin_arr();
    p.compaction_indexes
        .iter()
        .for_each(|&i| w.elem().int(usize_json(i)));
    w.end_arr();
    w.key("subagents").begin_arr();
    p.subagents.iter().for_each(|s| write_subagent(w.elem(), s));
    w.end_arr();
    opt(w, "cwd", p.cwd.as_ref());
    opt(w, "gitBranch", p.git_branch.as_ref());
    opt(w, "title", p.title.as_ref());
    opt(w, "version", p.version.as_ref());
    opt(w, "startedAt", p.started_at.as_ref());
    opt(w, "endedAt", p.ended_at.as_ref());
    if let Some(n) = p.native_context_window {
        w.key("nativeContextWindow").num(n);
    }
    write_counts(w, "unknownRecordTypes", p.unknown_record_types.as_ref());
    write_counts(
        w,
        "unknownSystemSubtypes",
        p.unknown_system_subtypes.as_ref(),
    );
    if let Some(c) = &p.sidechain_cost {
        w.key("sidechainCost").begin_obj();
        write_cost_fields(w, c);
        w.end_obj();
    }
    if let Some(n) = p.native_cost_usd {
        w.key("nativeCostUsd").num(n);
    }
}

/// `JSON.stringify(parsed, null, 2)`.
pub fn parsed_json(p: &ParsedSession) -> String {
    let mut w = Writer::new(true);
    w.begin_obj();
    write_parsed_fields(&mut w, p);
    w.end_obj();
    w.finish()
}

fn write_slices(w: &mut Writer, s: &BudgetSlices) {
    const ORDER: [BudgetKey; 10] = [
        BudgetKey::SystemTools,
        BudgetKey::Thinking,
        BudgetKey::AssistantText,
        BudgetKey::ToolResults,
        BudgetKey::Prompts,
        BudgetKey::Memory,
        BudgetKey::Listings,
        BudgetKey::Files,
        BudgetKey::Other,
        BudgetKey::Unattributed,
    ];
    w.begin_obj();
    for k in ORDER {
        w.key(k.as_str()).num(s.get(k));
    }
    w.end_obj();
}

fn write_context_file(w: &mut Writer, f: &OnDiskContextFile) {
    w.begin_obj();
    w.key("label").text(&f.label);
    w.key("path").text(&f.path);
    w.key("bytes").num(f.bytes);
    w.key("tokensEst").num(f.tokens_est);
    w.key("scope").str(f.scope.as_str());
    w.end_obj();
}

fn write_cost(w: &mut Writer, c: &SessionCost) {
    w.begin_obj();
    w.key("source")
        .str(if c.from_agent { "agent" } else { "estimate" });
    if let Some(v) = c.total_usd {
        w.key("totalUsd").num(v);
    }
    if let Some(v) = c.subagents_usd {
        w.key("subagentsUsd").num(v);
    }
    if let Some(m) = &c.unpriced_models {
        w.key("unpricedModels").texts(m.iter().map(String::as_str));
    }
    if let Some(n) = c.unreadable_subagents {
        w.key("unreadableSubagents").int(usize_json(n));
    }
    w.end_obj();
}

/// `JSON.stringify(analyzed, null, 2)`.
pub fn analyzed_json(a: &AnalyzedSession) -> String {
    let mut w = Writer::new(true);
    w.begin_obj();
    write_parsed_fields(&mut w, &a.parsed);
    w.key("contextWindow").num(a.context_window);
    w.key("contextWindowInferred")
        .bool(a.context_window_inferred);
    w.key("peakContextTokens").num(a.peak_context_tokens);
    w.key("peakTurnIndex").int(usize_json(a.peak_turn_index));
    w.key("finalContextTokens").num(a.final_context_tokens);
    w.key("totalOutputTokens").num(a.total_output_tokens);
    w.key("systemOverheadTokens").num(a.system_overhead_tokens);
    w.key("budget").begin_arr();
    for b in &a.budget {
        w.elem().begin_obj();
        w.key("key").str(b.key.as_str());
        w.key("label").str(b.label);
        w.key("short").str(b.short);
        w.key("color").str(b.color);
        w.key("estimated").bool(b.estimated);
        if let Some(note) = b.note {
            w.key("note").str(note);
        }
        w.key("tokens").num(b.tokens);
        w.end_obj();
    }
    w.end_arr();
    w.key("snapshots").begin_arr();
    for s in &a.snapshots {
        w.elem().begin_obj();
        w.key("turnIndex").int(usize_json(s.turn_index));
        w.key("model").text(&s.model);
        w.key("ctx").num(s.ctx);
        w.key("outputTokens").num(s.output_tokens);
        w.key("cacheReadTokens").num(s.cache_read_tokens);
        write_slices(w.key("slices"), &s.slices);
        if let Some(ts) = &s.ts {
            w.key("ts").text(ts);
        }
        w.end_obj();
    }
    w.end_arr();
    w.key("onDiskContextFiles").begin_arr();
    a.on_disk_context_files
        .iter()
        .for_each(|f| write_context_file(w.elem(), f));
    w.end_arr();
    w.key("dumbZoneCrossTurn").int(a.dumb_zone_cross_turn);
    w.key("dumbZoneFraction").num(a.dumb_zone_fraction);
    w.key("dumbZoneTurns").int(usize_json(a.dumb_zone_turns));
    w.key("compactionTurns").begin_arr();
    a.compaction_turns
        .iter()
        .for_each(|&t| w.elem().int(usize_json(t)));
    w.end_arr();
    w.key("turnCount").int(usize_json(a.turn_count));
    w.key("userMessageCount")
        .int(usize_json(a.user_message_count));
    w.key("toolCallCount").int(usize_json(a.tool_call_count));
    w.key("peakCacheReadTokens").num(a.peak_cache_read_tokens);
    w.key("webSearchRequests").num(a.web_search_requests);
    w.key("webFetchRequests").num(a.web_fetch_requests);
    if let Some(d) = a.duration_ms {
        w.key("durationMs").num(d);
    }
    if let Some(c) = &a.cost {
        write_cost(w.key("cost"), c);
    }
    w.end_obj();
    w.finish()
}
