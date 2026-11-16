//! Small analyses for unit tests.

use peektrace_protocol::view::{
    AnalyzedSession, EventKind, EventMeta, SessionMeta, TimelineEvent, Turn,
};

pub fn analysis() -> AnalyzedSession {
    AnalyzedSession {
        meta: SessionMeta {
            provider: "claude-code".into(),
            session_id: "11111111-2222".into(),
            path: "/tmp/s.jsonl".into(),
            models: vec!["model-a".into()],
            compaction_indexes: Vec::new(),
            subagents: Vec::new(),
            cwd: None,
            git_branch: None,
            title: None,
            context_window: 200_000.0,
            context_window_inferred: false,
            peak_context_tokens: 0.0,
            peak_turn_index: 0,
            total_output_tokens: 0.0,
            system_overhead_tokens: 0.0,
            budget: Vec::new(),
            dumb_zone_cross_turn: -1,
            dumb_zone_fraction: 0.4,
            dumb_zone_turns: 0,
            compaction_turns: Vec::new(),
            turn_count: 0,
            user_message_count: 0,
            tool_call_count: 0,
            peak_cache_read_tokens: 0.0,
            web_search_requests: 0.0,
            web_fetch_requests: 0.0,
            cost: None,
        },
        events: Vec::new(),
        turns: Vec::new(),
        snapshots: Vec::new(),
    }
}

/// An assistant text event; `request` empty for none.
pub fn event(request: &str) -> TimelineEvent {
    TimelineEvent {
        meta: EventMeta {
            index: 0,
            kind: EventKind::AssistantText,
            ts: None,
            title: String::new(),
            tokens_est: 0.0,
            tokens_measured: None,
            is_sidechain: None,
            is_error: None,
            is_meta: None,
            tool_name: None,
            tool_use_id: None,
            request_id: (!request.is_empty()).then(|| request.to_owned()),
            attachment_type: None,
            loaded_category: None,
            prompt_origin: None,
            prompt_source: None,
            thinking_duration_ms: None,
            server_tool: None,
            images: None,
            context_edit: None,
        },
        preview: String::new(),
        body: String::new(),
    }
}

pub fn kind(k: EventKind) -> TimelineEvent {
    let mut e = event("");
    e.kind = k;
    e
}

pub fn turn(request: &str) -> Turn {
    Turn {
        request_id: request.into(),
        model: "model-a".into(),
        context_tokens: 0.0,
        output_tokens: 0.0,
        thinking_tokens: None,
        effort: None,
        web_search_requests: None,
        web_fetch_requests: None,
        ts: None,
    }
}
