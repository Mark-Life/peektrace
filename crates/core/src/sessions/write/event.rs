//! JSON for events and turns: one key list per producer shape.

use crate::js::json_out::Writer;
use crate::sessions::model::{
    ContextEdit, EventShape, ImageRef, TimelineEvent, Turn, TurnField, TurnShape,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum F {
    Index,
    Kind,
    Ts,
    RequestId,
    IsSidechain,
    IsMeta,
    PromptOrigin,
    PromptSource,
    Title,
    Preview,
    Body,
    TokensEst,
    IsError,
    ServerTool,
    ThinkingDurationMs,
    ToolName,
    ToolUseId,
    AttachmentType,
    Images,
    LoadedCategory,
    ContextEdit,
    CompactPreTokens,
    CompactPostTokens,
}

use F::{
    AttachmentType, Body, CompactPostTokens, CompactPreTokens, Images, Index, IsError, IsMeta,
    IsSidechain, Kind, LoadedCategory, Preview, PromptOrigin, PromptSource, RequestId, ServerTool,
    ThinkingDurationMs, Title, TokensEst, ToolName, ToolUseId, Ts,
};

/// Keys each shape writes, in order. `tokensMeasured` always follows.
#[expect(clippy::too_many_lines, reason = "one key list per shape")]
const fn event_keys(shape: EventShape) -> &'static [F] {
    match shape {
        EventShape::ToolResult => &[
            Index,
            Kind,
            IsSidechain,
            Title,
            Preview,
            Body,
            TokensEst,
            IsError,
            Ts,
            Images,
            ToolUseId,
        ],
        EventShape::Prompt => &[
            Index,
            Kind,
            IsSidechain,
            Title,
            Preview,
            Body,
            TokensEst,
            PromptOrigin,
            Ts,
            IsMeta,
            PromptSource,
        ],
        EventShape::QueuedPrompt => &[
            Index,
            Kind,
            IsSidechain,
            Title,
            Preview,
            Body,
            TokensEst,
            PromptOrigin,
            Ts,
            IsMeta,
            PromptSource,
            AttachmentType,
        ],
        EventShape::Image => &[
            Index,
            Kind,
            IsSidechain,
            Title,
            Preview,
            Body,
            TokensEst,
            AttachmentType,
            LoadedCategory,
            Images,
            Ts,
        ],
        EventShape::Compaction => &[
            Index,
            Kind,
            IsSidechain,
            Title,
            Preview,
            Body,
            TokensEst,
            Ts,
            IsMeta,
            CompactPreTokens,
            CompactPostTokens,
        ],
        EventShape::AssistantBlock => &[
            Index,
            RequestId,
            IsSidechain,
            Ts,
            Kind,
            Title,
            Preview,
            Body,
            TokensEst,
            ThinkingDurationMs,
            ToolName,
            ServerTool,
            IsError,
            ToolUseId,
        ],
        EventShape::Summary => &[Index, Kind, Title, Preview, Body, TokensEst, Ts],
        EventShape::System => &[
            Index,
            Kind,
            IsSidechain,
            Title,
            Preview,
            Body,
            TokensEst,
            Ts,
        ],
        EventShape::Attachment => &[
            Index,
            Kind,
            IsSidechain,
            Title,
            Preview,
            Body,
            TokensEst,
            AttachmentType,
            LoadedCategory,
            Ts,
        ],
        EventShape::SystemPrompt => &[
            Index,
            Kind,
            Title,
            Preview,
            Body,
            TokensEst,
            AttachmentType,
            Ts,
            IsSidechain,
        ],
        EventShape::CodexText => &[Index, Ts, Kind, Title, Preview, Body, TokensEst, RequestId],
        EventShape::CodexAttachment => &[
            Index,
            Ts,
            Kind,
            Title,
            AttachmentType,
            LoadedCategory,
            Preview,
            Body,
            TokensEst,
        ],
        EventShape::CodexToolCall => &[
            Index, Ts, Kind, Title, Preview, Body, TokensEst, ToolName, ToolUseId, RequestId,
        ],
        EventShape::CodexToolResult => &[
            Index, Ts, Kind, Title, Preview, Body, TokensEst, IsError, ToolUseId,
        ],
        EventShape::PiPrompt => &[
            Index,
            Kind,
            Title,
            Preview,
            Body,
            TokensEst,
            Ts,
            F::ContextEdit,
        ],
        EventShape::PiAttachment => &[
            Index,
            Kind,
            Title,
            Preview,
            Body,
            TokensEst,
            AttachmentType,
            LoadedCategory,
            Ts,
            F::ContextEdit,
        ],
        EventShape::PiAssistantBlock => &[
            Index,
            RequestId,
            Ts,
            Kind,
            Title,
            Preview,
            Body,
            TokensEst,
            ToolName,
            ToolUseId,
            F::ContextEdit,
        ],
        EventShape::PiToolResult => &[
            Index,
            Kind,
            Title,
            Preview,
            Body,
            TokensEst,
            IsError,
            ToolName,
            ToolUseId,
            Ts,
            F::ContextEdit,
        ],
        EventShape::OcText => &[Index, Ts, RequestId, Kind, Title, Preview, Body, TokensEst],
        EventShape::OcToolCall => &[
            Index, Ts, RequestId, Kind, Title, Preview, Body, TokensEst, ToolName, ToolUseId,
        ],
        EventShape::OcToolResult => &[
            Index, Ts, RequestId, Kind, Title, Preview, Body, TokensEst, IsError, ToolName,
            ToolUseId,
        ],
        EventShape::OcAttachment => &[
            Index,
            Ts,
            RequestId,
            Kind,
            Title,
            AttachmentType,
            Preview,
            Body,
            TokensEst,
        ],
        EventShape::OcSystem => &[
            Index, Ts, RequestId, Kind, Title, Preview, Body, TokensEst, ToolName,
        ],
    }
}

#[expect(clippy::cast_possible_wrap, reason = "positions fit i64")]
pub(super) fn usize_json(n: usize) -> i64 {
    n as i64
}

fn write_images(w: &mut Writer, images: &[ImageRef]) {
    w.begin_arr();
    for img in images {
        w.elem().begin_obj();
        w.key("block").int(usize_json(img.block));
        w.key("mediaType").text(&img.media_type);
        if let Some(item) = img.item {
            w.key("item").int(usize_json(item));
        }
        w.end_obj();
    }
    w.end_arr();
}

fn write_context_edit(w: &mut Writer, edit: &ContextEdit, with_original: bool) {
    w.begin_obj();
    w.key("status").str(edit.status());
    w.key("atIndex").num(edit.at_index());
    let steps = match edit {
        ContextEdit::Removed { steps, .. } => steps,
        ContextEdit::Replaced {
            original_tokens_est,
            original,
            steps,
            ..
        } => {
            w.key("originalTokensEst").num(*original_tokens_est);
            if let Some(o) = original.as_ref().filter(|_| with_original) {
                w.key("original").text(o);
            }
            steps
        }
    };
    if let Some(steps) = steps {
        w.key("steps").begin_arr();
        for s in steps {
            w.elem().begin_obj();
            w.key("atIndex").num(s.at_index);
            w.key("tokensEst").num(s.tokens_est);
            w.key("thinkingEst").num(s.thinking_est);
            w.end_obj();
        }
        w.end_arr();
    }
    w.end_obj();
}

fn write_field(w: &mut Writer, e: &TimelineEvent, f: F, row: bool) {
    let text = |w: &mut Writer, key: &str, v: Option<&String>| {
        if let Some(v) = v {
            w.key(key).text(v);
        }
    };
    let flag = |w: &mut Writer, key: &str, v: Option<bool>| {
        if let Some(v) = v {
            w.key(key).bool(v);
        }
    };
    let num = |w: &mut Writer, key: &str, v: Option<f64>| {
        if let Some(v) = v {
            w.key(key).num(v);
        }
    };
    match f {
        Index => w.key("index").int(usize_json(e.index)),
        Kind => w.key("kind").str(e.kind.as_str()),
        Ts => text(w, "ts", e.ts.as_ref()),
        RequestId => text(w, "requestId", e.request_id.as_ref()),
        IsSidechain => flag(w, "isSidechain", e.is_sidechain),
        IsMeta => flag(w, "isMeta", e.is_meta),
        PromptOrigin => {
            if let Some(o) = e.prompt_origin {
                w.key("promptOrigin").str(o.as_str());
            }
        }
        PromptSource => text(w, "promptSource", e.prompt_source.as_ref()),
        Title => w.key("title").text(&e.title),
        Preview => w.key("preview").text(&e.preview),
        Body => w.key("body").text(&e.body),
        TokensEst => w.key("tokensEst").num(e.tokens_est),
        IsError => flag(w, "isError", e.is_error),
        ServerTool => flag(w, "serverTool", e.server_tool),
        ThinkingDurationMs => num(w, "thinkingDurationMs", e.thinking_duration_ms),
        ToolName => text(w, "toolName", e.tool_name.as_ref()),
        ToolUseId => text(w, "toolUseId", e.tool_use_id.as_ref()),
        AttachmentType => text(w, "attachmentType", e.attachment_type.as_ref()),
        Images => {
            if let Some(images) = &e.images {
                write_images(w.key("images"), images);
            }
        }
        LoadedCategory => {
            if let Some(c) = e.loaded_category {
                w.key("loadedCategory").str(c.as_str());
            }
        }
        F::ContextEdit => {
            if let Some(edit) = &e.context_edit {
                write_context_edit(w.key("contextEdit"), edit, !row);
            }
        }
        CompactPreTokens => num(w, "compactPreTokens", e.compact_pre_tokens),
        CompactPostTokens => num(w, "compactPostTokens", e.compact_post_tokens),
    }
}

pub fn write_event(w: &mut Writer, e: &TimelineEvent) {
    w.begin_obj();
    write_event_fields(w, e, false);
    w.end_obj();
}

/// An event's keys, no braces. A `row` (history index) leaves out the
/// body, preview, request id, timestamp and context-edit original.
pub fn write_event_fields(w: &mut Writer, e: &TimelineEvent, row: bool) {
    let dropped = |f: &F| row && matches!(f, Body | Preview | RequestId | Ts);
    for &f in event_keys(e.shape).iter().filter(|f| !dropped(f)) {
        write_field(w, e, f, row);
    }
    if let Some(m) = e.tokens_measured {
        w.key("tokensMeasured").num(m);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum T {
    RequestId,
    Model,
    ContextTokens,
    InputTokens,
    CacheReadTokens,
    CacheCreationTokens,
    OutputTokens,
    CacheCreation1h,
    CacheCreation5m,
    EventIndexes,
    Ts,
    Effort,
    Late(TurnField),
}

const fn turn_keys(shape: TurnShape) -> &'static [T] {
    match shape {
        TurnShape::Claude => &[
            T::RequestId,
            T::Model,
            T::ContextTokens,
            T::InputTokens,
            T::CacheReadTokens,
            T::CacheCreationTokens,
            T::OutputTokens,
            T::CacheCreation1h,
            T::CacheCreation5m,
            T::EventIndexes,
            T::Ts,
            T::Late(TurnField::ThinkingTokens),
            T::Late(TurnField::WebSearchRequests),
            T::Late(TurnField::WebFetchRequests),
            T::Effort,
        ],
        TurnShape::Opencode => &[
            T::Model,
            T::InputTokens,
            T::CacheReadTokens,
            T::CacheCreationTokens,
            T::RequestId,
            T::ContextTokens,
            T::OutputTokens,
            T::EventIndexes,
            T::Ts,
            T::Late(TurnField::CostUsd),
        ],
    }
}

fn write_late(w: &mut Writer, t: &Turn, f: TurnField) {
    let (key, v) = match f {
        TurnField::ThinkingTokens => ("thinkingTokens", t.thinking_tokens),
        TurnField::WebSearchRequests => ("webSearchRequests", t.web_search_requests),
        TurnField::WebFetchRequests => ("webFetchRequests", t.web_fetch_requests),
        TurnField::CostUsd => ("costUsd", t.cost_usd),
    };
    if let Some(v) = v {
        w.key(key).num(v);
    }
}

pub fn write_turn(w: &mut Writer, t: &Turn) {
    w.begin_obj();
    for &k in turn_keys(t.shape) {
        match k {
            T::RequestId => w.key("requestId").text(&t.request_id),
            T::Model => w.key("model").text(&t.model),
            T::ContextTokens => w.key("contextTokens").num(t.context_tokens),
            T::InputTokens => w.key("inputTokens").num(t.input_tokens),
            T::CacheReadTokens => w.key("cacheReadTokens").num(t.cache_read_tokens),
            T::CacheCreationTokens => w.key("cacheCreationTokens").num(t.cache_creation_tokens),
            T::OutputTokens => w.key("outputTokens").num(t.output_tokens),
            T::CacheCreation1h => {
                if let Some(v) = t.cache_creation_1h_tokens {
                    w.key("cacheCreation1hTokens").num(v);
                }
            }
            T::CacheCreation5m => {
                if let Some(v) = t.cache_creation_5m_tokens {
                    w.key("cacheCreation5mTokens").num(v);
                }
            }
            T::EventIndexes => {
                w.key("eventIndexes").begin_arr();
                t.event_indexes.iter().for_each(|&i| w.elem().int(i));
                w.end_arr();
            }
            T::Ts => {
                if let Some(ts) = &t.ts {
                    w.key("ts").text(ts);
                }
            }
            T::Effort => {
                if let Some(e) = &t.effort {
                    w.key("effort").text(e);
                }
            }
            T::Late(f) if !t.appended.contains(&f) => write_late(w, t, f),
            T::Late(_) => {}
        }
    }
    t.appended.iter().for_each(|&f| write_late(w, t, f));
    w.end_obj();
}
