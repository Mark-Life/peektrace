//! A parsed session read back from the JSON [`super::write::parsed_json`]
//! writes. Key order carries the producer shape of each event and turn, so
//! the reader recovers it: writing the result again gives the same text.

use crate::js::json_out::Writer;
use crate::js::value::{JsObject, JsValue};
use crate::sessions::model::{
    ContextEdit, ContextEditStep, EventKind, EventShape, ImageRef, LoadedCategory, ParsedSession,
    PromptOrigin, Provider, SubagentRef, TimelineEvent, Turn, TurnField, TurnShape, TurnTokens,
    TurnsCost,
};
use crate::sessions::write::{write_event, write_turn};

fn s(v: &JsValue, k: &str) -> Option<String> {
    v.get(k).and_then(JsValue::as_str).map(str::to_owned)
}

fn n(v: &JsValue, k: &str) -> Option<f64> {
    v.get(k).and_then(JsValue::as_num)
}

fn b(v: &JsValue, k: &str) -> Option<bool> {
    match v.get(k) {
        Some(JsValue::Bool(x)) => Some(*x),
        _ => None,
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "indexes are non-negative integers"
)]
fn idx(v: &JsValue, k: &str) -> Option<usize> {
    n(v, k).filter(|x| *x >= 0.0).map(|x| x as usize)
}

fn strings(v: &JsValue, k: &str) -> Option<Vec<String>> {
    v.get(k)?
        .as_arr()?
        .iter()
        .map(|x| x.as_str().map(str::to_owned))
        .collect()
}

fn named<T: Copy>(all: &[T], name: &str, as_str: impl Fn(T) -> &'static str) -> Option<T> {
    all.iter().copied().find(|x| as_str(*x) == name)
}

fn keys(v: &JsValue) -> Vec<&str> {
    v.as_obj().map(|o| o.keys().collect()).unwrap_or_default()
}

fn image(v: &JsValue) -> Option<ImageRef> {
    Some(ImageRef {
        block: idx(v, "block")?,
        media_type: s(v, "mediaType")?,
        item: idx(v, "item"),
    })
}

fn steps(v: &JsValue) -> Option<Vec<ContextEditStep>> {
    v.get("steps")?
        .as_arr()?
        .iter()
        .map(|x| {
            Some(ContextEditStep {
                at_index: n(x, "atIndex")?,
                tokens_est: n(x, "tokensEst")?,
                thinking_est: n(x, "thinkingEst")?,
            })
        })
        .collect()
}

fn context_edit(v: &JsValue) -> Option<ContextEdit> {
    let at_index = n(v, "atIndex")?;
    match v.get("status")?.as_str()? {
        "removed" => Some(ContextEdit::Removed {
            at_index,
            steps: steps(v),
        }),
        "replaced" => Some(ContextEdit::Replaced {
            at_index,
            original_tokens_est: n(v, "originalTokensEst")?,
            original: s(v, "original"),
            steps: steps(v),
        }),
        _ => None,
    }
}

fn event_fields(v: &JsValue, shape: EventShape) -> Option<TimelineEvent> {
    let kind = named(&EventKind::ALL, v.get("kind")?.as_str()?, EventKind::as_str)?;
    let mut e = TimelineEvent::new(shape, idx(v, "index")?, kind);
    e.ts = s(v, "ts");
    e.request_id = s(v, "requestId");
    e.is_sidechain = b(v, "isSidechain");
    e.is_meta = b(v, "isMeta");
    e.prompt_origin = match v.get("promptOrigin") {
        None => None,
        Some(o) => Some(named(
            &PromptOrigin::ALL,
            o.as_str()?,
            PromptOrigin::as_str,
        )?),
    };
    e.prompt_source = s(v, "promptSource");
    e.title = s(v, "title").unwrap_or_default();
    e.preview = s(v, "preview").unwrap_or_default();
    e.body = s(v, "body").unwrap_or_default();
    e.tokens_est = n(v, "tokensEst").unwrap_or_default();
    e.tokens_measured = n(v, "tokensMeasured");
    e.is_error = b(v, "isError");
    e.server_tool = b(v, "serverTool");
    e.thinking_duration_ms = n(v, "thinkingDurationMs");
    e.tool_name = s(v, "toolName");
    e.tool_use_id = s(v, "toolUseId");
    e.attachment_type = s(v, "attachmentType");
    e.images = match v.get("images") {
        None => None,
        Some(list) => Some(list.as_arr()?.iter().map(image).collect::<Option<_>>()?),
    };
    e.loaded_category = match v.get("loadedCategory") {
        None => None,
        Some(c) => Some(named(
            &LoadedCategory::ALL,
            c.as_str()?,
            LoadedCategory::as_str,
        )?),
    };
    e.context_edit = match v.get("contextEdit") {
        None => None,
        Some(c) => Some(context_edit(c)?),
    };
    e.compact_pre_tokens = n(v, "compactPreTokens");
    e.compact_post_tokens = n(v, "compactPostTokens");
    Some(e)
}

fn written<T>(x: &T, write: impl Fn(&mut Writer, &T)) -> String {
    let mut w = Writer::new(false);
    write(&mut w, x);
    w.finish()
}

/// The event, with the first shape whose key order matches the text.
fn event(v: &JsValue) -> Option<TimelineEvent> {
    let want = v.stringify(false);
    let first = event_fields(v, EventShape::ALL[0])?;
    let matched = EventShape::ALL.iter().find_map(|&shape| {
        let e = TimelineEvent {
            shape,
            ..first.clone()
        };
        (written(&e, write_event) == want).then_some(e)
    });
    Some(matched.unwrap_or(first))
}

const LATE: [(TurnField, &str); 4] = [
    (TurnField::ThinkingTokens, "thinkingTokens"),
    (TurnField::WebSearchRequests, "webSearchRequests"),
    (TurnField::WebFetchRequests, "webFetchRequests"),
    (TurnField::CostUsd, "costUsd"),
];

fn turn(v: &JsValue) -> Option<Turn> {
    let shape = if keys(v).first() == Some(&"model") {
        TurnShape::Opencode
    } else {
        TurnShape::Claude
    };
    let tokens = TurnTokens {
        context: n(v, "contextTokens")?,
        input: n(v, "inputTokens")?,
        cache_read: n(v, "cacheReadTokens")?,
        cache_creation: n(v, "cacheCreationTokens")?,
        output: n(v, "outputTokens")?,
    };
    let mut t = Turn::new(s(v, "requestId")?, s(v, "model")?, tokens, s(v, "ts"));
    t.shape = shape;
    t.cache_creation_1h_tokens = n(v, "cacheCreation1hTokens");
    t.cache_creation_5m_tokens = n(v, "cacheCreation5mTokens");
    #[expect(clippy::cast_possible_truncation, reason = "positions are integers")]
    let positions = v
        .get("eventIndexes")?
        .as_arr()?
        .iter()
        .map(|x| x.as_num().map(|f| f as i64))
        .collect::<Option<Vec<_>>>()?;
    t.event_indexes = positions;
    t.thinking_tokens = n(v, "thinkingTokens");
    t.web_search_requests = n(v, "webSearchRequests");
    t.web_fetch_requests = n(v, "webFetchRequests");
    t.effort = s(v, "effort");
    t.cost_usd = n(v, "costUsd");
    // Late keys written after the base keys are a suffix of the late keys.
    let want = v.stringify(false);
    let late: Vec<TurnField> = keys(v)
        .iter()
        .filter_map(|k| LATE.iter().find(|(_, name)| name == k).map(|(f, _)| *f))
        .collect();
    for from in 0..=late.len() {
        t.appended = late[from..].to_vec();
        if written(&t, write_turn) == want {
            break;
        }
    }
    Some(t)
}

fn cost(v: &JsValue) -> TurnsCost {
    TurnsCost {
        cost_usd: n(v, "costUsd"),
        unpriced_models: strings(v, "unpricedModels"),
    }
}

fn subagent(v: &JsValue) -> Option<SubagentRef> {
    let ks = keys(v);
    let at = |k: &str| ks.iter().position(|x| *x == k);
    let linked = at("toolUseId").is_some_and(|t| {
        [
            "parentAgentId",
            "workflowRunId",
            "workflowPhase",
            "workflowPhaseIndex",
        ]
        .iter()
        .filter_map(|k| at(k))
        .any(|i| i < t)
    });
    Some(SubagentRef {
        id: s(v, "id")?,
        path: s(v, "path")?,
        agent_type: s(v, "agentType"),
        description: s(v, "description"),
        tool_use_id: s(v, "toolUseId"),
        parent_agent_id: s(v, "parentAgentId"),
        workflow_run_id: s(v, "workflowRunId"),
        workflow_phase: s(v, "workflowPhase"),
        workflow_phase_index: n(v, "workflowPhaseIndex"),
        tool_use_id_linked: linked,
        turns: idx(v, "turns")?,
        peak_context_tokens: n(v, "peakContextTokens")?,
        cost: cost(v),
        unreadable: b(v, "unreadable") == Some(true),
    })
}

fn counts(o: &JsObject) -> Option<Vec<(String, f64)>> {
    o.entries()
        .map(|(k, v)| Some((k.to_owned(), v.as_num()?)))
        .collect()
}

fn list<T>(v: &JsValue, k: &str, f: impl Fn(&JsValue) -> Option<T>) -> Option<Vec<T>> {
    v.get(k)?.as_arr()?.iter().map(f).collect()
}

/// A parsed session, as [`super::write::parsed_json`] writes it.
pub fn parsed_from_json(v: &JsValue) -> Option<ParsedSession> {
    let count_map = |k: &str| match v.get(k) {
        None => Some(None),
        Some(o) => counts(o.as_obj()?).map(Some),
    };
    Some(ParsedSession {
        provider: named(
            &Provider::ALL,
            v.get("provider")?.as_str()?,
            Provider::as_str,
        )?,
        session_id: s(v, "sessionId")?,
        path: s(v, "path")?,
        models: strings(v, "models")?,
        events: list(v, "events", event)?,
        turns: list(v, "turns", turn)?,
        compaction_indexes: list(v, "compactionIndexes", |x| {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "indexes are non-negative integers"
            )]
            x.as_num().filter(|f| *f >= 0.0).map(|f| f as usize)
        })?,
        subagents: list(v, "subagents", subagent)?,
        cwd: s(v, "cwd"),
        git_branch: s(v, "gitBranch"),
        title: s(v, "title"),
        version: s(v, "version"),
        started_at: s(v, "startedAt"),
        ended_at: s(v, "endedAt"),
        native_context_window: n(v, "nativeContextWindow"),
        unknown_record_types: count_map("unknownRecordTypes")?,
        unknown_system_subtypes: count_map("unknownSystemSubtypes")?,
        sidechain_cost: v.get("sidechainCost").map(cost),
        native_cost_usd: n(v, "nativeCostUsd"),
    })
}
