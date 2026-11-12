//! An `OpenCode` session to the parsed-session model: each message opens a
//! turn when the assistant wrote it, and each part becomes timeline events.

use super::session::{Session, SessionInfo};
use crate::js::time;
use crate::js::units;
use crate::js::value::{JsObject, JsValue, str_of};
use crate::pricing::{CostTokens, card_for, turn_cost};
use crate::sessions::measure::measure_tokens;
use crate::sessions::model::{
    EventKind, EventShape, ParsedSession, Provider, TimelineEvent, Turn, TurnShape,
};

const LINE_CAP: usize = 200;
/// `new Date(ms)` is valid up to this many ms either side of the epoch.
const MAX_TIME: f64 = 8.64e15;

/// `typeof v === "number" ? v : 0`.
pub(super) fn num(v: Option<&JsValue>) -> f64 {
    v.and_then(JsValue::as_num).unwrap_or(0.0)
}

/// A finite number above 0. Non-finite values are `null` once the TS
/// reader's JSON round trip is done.
pub(super) fn positive(v: f64) -> Option<f64> {
    (v.is_finite() && v > 0.0).then_some(v)
}

/// `new Date(ms).toISOString()` for a positive time. TS throws past
/// `8.64e15`; such a time reads as no date here.
pub(super) fn iso_from_ms(ms: f64) -> Option<String> {
    #[expect(clippy::cast_possible_truncation, reason = "bounded by MAX_TIME")]
    (ms.is_finite() && ms > 0.0 && ms <= MAX_TIME).then(|| time::to_iso_string(ms.trunc() as i64))
}

/// A plain-object field.
fn obj<'a>(o: &'a JsObject, key: &str) -> Option<&'a JsObject> {
    o.get(key).and_then(JsValue::as_obj)
}

fn string(o: &JsObject, key: &str) -> Option<String> {
    o.get(key).and_then(JsValue::as_str).map(str::to_owned)
}

/// The id a session `model` names: a string (or JSON text of an object with
/// a string `id`), or an object's string `id`.
pub(super) fn model_id(model: Option<&JsValue>) -> Option<String> {
    match model? {
        JsValue::Str(s) => Some(
            JsValue::parse(s)
                .and_then(|v| v.get("id").and_then(JsValue::as_str).map(str::to_owned))
                .unwrap_or_else(|| s.clone()),
        ),
        JsValue::Obj(o) => string(o, "id"),
        _ => None,
    }
    .filter(|m| !m.is_empty())
}

/// The model on a message: `modelID`, else `model.modelID`, else `model.id`.
fn message_model(msg: &JsObject) -> Option<String> {
    string(msg, "modelID").or_else(|| {
        let model = obj(msg, "model")?;
        string(model, "modelID").or_else(|| string(model, "id"))
    })
}

fn message_created(o: &JsObject) -> Option<f64> {
    obj(o, "time")?.get("created")?.as_num()
}

/// The turn an assistant message makes. `context` is the running token
/// estimate, used when the message has no usage.
pub(super) fn turn_for_message(
    msg: &JsObject,
    ts: Option<String>,
    session_model: Option<&str>,
    context: f64,
) -> Turn {
    let model = message_model(msg)
        .or_else(|| session_model.map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned());
    let tokens = obj(msg, "tokens");
    let token = |k: &str| num(tokens.and_then(|t| t.get(k)));
    let cache = tokens.and_then(|t| obj(t, "cache"));
    let cache_token = |k: &str| num(cache.and_then(|c| c.get(k)));
    let (input, output) = (token("input"), token("output"));
    let (cache_read, cache_creation) = (cache_token("read"), cache_token("write"));
    let ground = input + cache_read + cache_creation;
    let cost_usd = positive(num(msg.get("cost"))).or_else(|| {
        turn_cost(
            card_for(&model).as_ref(),
            &CostTokens {
                input,
                output: output + token("reasoning"),
                cache_read,
                cache_creation,
                cache_creation_1h: None,
            },
        )
    });
    Turn {
        shape: TurnShape::Opencode,
        request_id: string(msg, "id").unwrap_or_default(),
        model,
        context_tokens: if ground > 0.0 { ground } else { context },
        input_tokens: input,
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_creation,
        output_tokens: output,
        cache_creation_1h_tokens: None,
        cache_creation_5m_tokens: None,
        event_indexes: Vec::new(),
        ts,
        thinking_tokens: None,
        web_search_requests: None,
        web_fetch_requests: None,
        effort: None,
        cost_usd,
        appended: Vec::new(),
    }
}

/// True when a turn billed any token (reasoning aside).
pub(super) fn billed(t: &Turn) -> bool {
    t.input_tokens + t.cache_read_tokens + t.cache_creation_tokens + t.output_tokens > 0.0
}

/// The session `cost` when above 0, else the sum of message costs when every
/// message that billed tokens has one.
pub(super) fn agent_cost(session_cost: Option<f64>, message_costs: &[Option<f64>]) -> Option<f64> {
    if session_cost.is_some() {
        return session_cost;
    }
    if message_costs.is_empty() {
        return None;
    }
    message_costs
        .iter()
        .try_fold(0.0, |sum, c| c.map(|c| sum + c))
}

/// `JSON.stringify(v, null, 2)`, `""` for nullish.
fn pretty(v: Option<&JsValue>) -> String {
    match v {
        None | Some(JsValue::Null) => String::new(),
        Some(v) => v.stringify(true),
    }
}

/// Fields every event of one part shares.
struct Base {
    ts: Option<String>,
    request_id: Option<String>,
}

fn event(
    shape: EventShape,
    base: &Base,
    kind: EventKind,
    title: String,
    body: String,
) -> TimelineEvent {
    let preview = units::first_line(&body, LINE_CAP);
    let est = units::est_tokens(&body);
    let mut e = TimelineEvent::new(shape, 0, kind).text(title, preview, body, est);
    e.ts.clone_from(&base.ts);
    e.request_id.clone_from(&base.request_id);
    e
}

fn tool_events(part: &JsObject, base: &Base) -> Vec<TimelineEvent> {
    let name = Some(str_of(part.get("tool")))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "tool".to_owned());
    let call_id = string(part, "callID");
    let empty = JsObject::default();
    let state = obj(part, "state").unwrap_or(&empty);
    let input = pretty(state.get("input"));
    let mut call = event(
        EventShape::OcToolCall,
        base,
        EventKind::ToolCall,
        name.clone(),
        input,
    );
    call.preview = units::first_line(&units::collapse_ws(&call.body), LINE_CAP);
    call.tool_name = Some(name.clone());
    call.tool_use_id.clone_from(&call_id);
    let status = str_of(state.get("status"));
    let output = state.get("output").and_then(JsValue::as_str);
    if !(status == "completed" || status == "error" || output.is_some()) {
        return vec![call];
    }
    let is_error = status == "error";
    let body = output.map_or_else(
        || JsValue::Obj(state.clone()).stringify(true),
        str::to_owned,
    );
    let title = if is_error {
        "tool_result (error)"
    } else {
        "tool_result"
    };
    let mut result = event(
        EventShape::OcToolResult,
        base,
        EventKind::ToolResult,
        title.to_owned(),
        body,
    );
    result.is_error = Some(is_error);
    result.tool_name = Some(name);
    result.tool_use_id = call_id;
    vec![call, result]
}

/// The events one part makes, given the role of its message.
fn part_events(part: &JsObject, role: &str, base: &Base) -> Vec<TimelineEvent> {
    let kind = str_of(part.get("type"));
    let whole = || JsValue::Obj(part.clone()).stringify(true);
    let one =
        |shape, kind, title: &str, body| vec![event(shape, base, kind, title.to_owned(), body)];
    match kind.as_str() {
        "text" => {
            let text = str_of(part.get("text"));
            let (kind, title) = if role == "user" {
                (EventKind::UserPrompt, "User prompt")
            } else {
                (EventKind::AssistantText, "Assistant")
            };
            one(EventShape::OcText, kind, title, text)
        }
        "reasoning" => {
            let mut e = one(
                EventShape::OcText,
                EventKind::AssistantThinking,
                "Thinking",
                str_of(part.get("text")),
            );
            if e[0].body.is_empty() {
                "(reasoning not stored)".clone_into(&mut e[0].preview);
            }
            e
        }
        "tool" => tool_events(part, base),
        "file" => {
            let filename = str_of(part.get("filename"));
            let title = if filename.is_empty() {
                "file"
            } else {
                &filename
            };
            let mut e = one(
                EventShape::OcAttachment,
                EventKind::Attachment,
                title,
                whole(),
            );
            if !filename.is_empty() {
                e[0].preview = units::first_line(&filename, LINE_CAP);
            }
            e[0].attachment_type = Some("file".to_owned());
            e
        }
        "patch" | "snapshot" => {
            let mut e = one(
                EventShape::OcAttachment,
                EventKind::Attachment,
                &kind,
                whole(),
            );
            e[0].attachment_type = Some(kind.clone());
            e
        }
        "subtask" | "agent" => {
            let title = if kind == "subtask" {
                "subagent spawn"
            } else {
                "agent"
            };
            let mut e = one(EventShape::OcSystem, EventKind::System, title, whole());
            e[0].tool_name = string(part, "tool");
            e
        }
        "retry" => one(EventShape::OcSystem, EventKind::System, "retry", whole()),
        "compaction" => one(
            EventShape::OcText,
            EventKind::Compaction,
            "Compaction",
            whole(),
        ),
        _ => Vec::new(),
    }
}

/// Session-line facts the parse keeps.
struct Meta {
    started_at: Option<String>,
    ended_at: Option<String>,
    native_cost: Option<f64>,
    session_model: Option<String>,
}

fn meta_of(info: &SessionInfo) -> Meta {
    Meta {
        started_at: iso_from_ms(info.time_created),
        ended_at: iso_from_ms(info.time_updated),
        native_cost: positive(info.cost),
        session_model: model_id(info.model.as_ref()),
    }
}

fn push_model(models: &mut Vec<String>, m: Option<String>) {
    if let Some(m) = m.filter(|m| !m.is_empty() && !models.contains(m)) {
        models.push(m);
    }
}

/// Parse a loaded session. `None` (not found) gives an empty session.
pub fn parse_opencode_session(
    session: Option<&Session>,
    path: &str,
    session_id: &str,
) -> ParsedSession {
    let mut p = ParsedSession {
        provider: Provider::Opencode,
        session_id: session_id.to_owned(),
        path: path.to_owned(),
        models: Vec::new(),
        events: Vec::new(),
        turns: Vec::new(),
        compaction_indexes: Vec::new(),
        subagents: Vec::new(),
        cwd: None,
        git_branch: None,
        title: None,
        version: None,
        started_at: None,
        ended_at: None,
        native_context_window: None,
        unknown_record_types: None,
        unknown_system_subtypes: None,
        sidechain_cost: None,
        native_cost_usd: None,
    };
    let Some(session) = session else {
        return p;
    };
    let info = &session.info;
    let meta = meta_of(info);
    p.session_id.clone_from(&info.id);
    p.cwd.clone_from(&info.directory);
    p.title.clone_from(&info.title);
    p.version.clone_from(&info.version);
    push_model(&mut p.models, meta.session_model.clone());
    let mut started_at = meta.started_at;
    let mut last_message_at = None;
    let mut message_costs = Vec::new();
    let mut context = 0.0;
    for m in &session.messages {
        let msg = &m.data;
        let role = str_of(msg.get("role"));
        let created = message_created(msg);
        let ts = created.and_then(iso_from_ms);
        if let Some(ts) = &ts {
            started_at.get_or_insert_with(|| ts.clone());
            last_message_at = Some(ts.clone());
        }
        push_model(&mut p.models, message_model(msg));
        let turn = (role == "assistant").then(|| {
            let t = turn_for_message(msg, ts, meta.session_model.as_deref(), context);
            if billed(&t) {
                message_costs.push(positive(num(msg.get("cost"))));
            }
            p.turns.push(t);
            p.turns.len() - 1
        });
        for part in &m.parts {
            let base = Base {
                ts: message_created(part).or(created).and_then(iso_from_ms),
                request_id: turn.map(|t| p.turns[t].request_id.clone()),
            };
            for mut e in part_events(part, &role, &base) {
                let at = p.events.len();
                e.index = at;
                context += e.tokens_est;
                if let Some(t) = turn {
                    p.turns[t]
                        .event_indexes
                        .push(i64::try_from(at).unwrap_or(i64::MAX));
                }
                if e.kind == EventKind::Compaction {
                    p.compaction_indexes.push(at);
                }
                p.events.push(e);
            }
        }
    }
    p.started_at = started_at;
    p.ended_at = meta.ended_at.or(last_message_at);
    p.native_cost_usd = agent_cost(meta.native_cost, &message_costs);
    measure_tokens(&mut p);
    p
}
