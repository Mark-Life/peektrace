//! Labels for events and sessions. Copies of `peektrace_core::sessions`
//! label and cost helpers over the wire types; `tests/drift.rs` keeps them
//! equal to core.

use peektrace_protocol::view::{
    AnalyzedSession, CostSource, EventKind, PromptOrigin, SessionCost, TimelineEvent,
};

use super::jsnum::{to_fixed, to_js_string};

/// The badge label of one row: tool name, `system prompt`, who wrote a
/// prompt the user did not type, the attachment type as words, else the kind.
pub fn event_badge_label(e: &TimelineEvent) -> String {
    if let Some(name) = &e.tool_name {
        return name.clone();
    }
    if e.kind == EventKind::SystemPrompt {
        return "system prompt".to_owned();
    }
    if e.kind == EventKind::UserPrompt
        && let Some(origin) = e.prompt_origin
        && !matches!(origin, PromptOrigin::Human | PromptOrigin::Sdk)
    {
        return origin_str(origin).replace('-', " ");
    }
    let ty = e.attachment_type.as_deref().map_or("", js_trim);
    if ty.is_empty() {
        e.kind.as_str().to_owned()
    } else {
        ty.replace('_', " ")
    }
}

const fn origin_str(o: PromptOrigin) -> &'static str {
    match o {
        PromptOrigin::Human => "human",
        PromptOrigin::Sdk => "sdk",
        PromptOrigin::Command => "command",
        PromptOrigin::TaskNotification => "task-notification",
        PromptOrigin::Scheduled => "scheduled",
        PromptOrigin::Harness => "harness",
    }
}

/// `String.prototype.trim`: JS white space and line terminators.
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c == '\u{FEFF}' || (c.is_whitespace() && c != '\u{0085}'))
}

/// An event's best known size and whether usage measured it.
pub fn event_tokens(e: &TimelineEvent) -> (f64, bool) {
    e.tokens_measured
        .map_or((e.tokens_est, false), |m| (m, true))
}

/// `~N` for an estimate, `N` when measured.
pub fn token_label(e: &TimelineEvent, format: impl Fn(f64) -> String) -> String {
    let (tokens, measured) = event_tokens(e);
    format!("{}{}", if measured { "" } else { "~" }, format(tokens))
}

/// What to say about a thinking block whose text the transcript did not keep.
pub fn thinking_note(e: &TimelineEvent) -> String {
    match e.tokens_measured {
        None => "Thinking content is not stored in the transcript (only a signature). Its token cost is in the timeline 'thinking' band.".to_owned(),
        Some(n) => format!(
            "Thinking text is not stored in the transcript (only a signature). Usage reports {} thinking tokens for it; they stay in the context window on later turns.",
            to_js_string(n)
        ),
    }
}

/// Display label of a transcript provider.
pub fn provider_label(provider: &str) -> &str {
    match provider {
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        "pi" => "Pi",
        "opencode" => "OpenCode",
        other => other,
    }
}

/// The session's name: its title, else `<Provider> session`.
pub fn analyzed_title(a: &AnalyzedSession) -> String {
    match a.title.as_deref() {
        Some(t) if !t.is_empty() => t.to_owned(),
        _ => format!("{} session", provider_label(&a.provider)),
    }
}

const CENT: f64 = 0.005;

/// `~$1.23`, or `<$0.01` below half a cent.
pub fn fmt_usd(n: f64) -> String {
    if n > 0.0 && n < CENT {
        "<$0.01".to_owned()
    } else {
        format!("~${}", to_fixed(n, 2))
    }
}

/// A session cost as display text: the total (`—` when nothing is priced)
/// and the notes that qualify it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostSummary {
    pub total: String,
    pub notes: Vec<String>,
}

pub fn cost_summary(cost: &SessionCost, agent_label: &str) -> CostSummary {
    let mut notes = Vec::new();
    if cost.source == CostSource::Agent {
        notes.push(format!("reported by {agent_label}"));
    }
    if let Some(s) = cost.subagents_usd.filter(|s| *s != 0.0 && !s.is_nan()) {
        notes.push(format!("subagents {}", fmt_usd(s)));
    }
    if let Some(models) = cost.unpriced_models.as_ref().filter(|m| !m.is_empty()) {
        notes.push(format!("unpriced: {}", models.join(", ")));
    }
    if let Some(n) = cost.unreadable_subagents.filter(|n| *n > 0) {
        notes.push(format!("{n} subagent(s) unreadable"));
    }
    CostSummary {
        total: cost.total_usd.map_or_else(|| "—".to_owned(), fmt_usd),
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost(source: CostSource) -> SessionCost {
        SessionCost {
            source,
            total_usd: None,
            subagents_usd: None,
            unpriced_models: None,
            unreadable_subagents: None,
        }
    }

    #[test]
    fn cost_text() {
        let c = SessionCost {
            total_usd: Some(1.234),
            ..cost(CostSource::Estimate)
        };
        assert_eq!(
            cost_summary(&c, "Claude Code"),
            CostSummary {
                total: "~$1.23".into(),
                notes: vec![]
            }
        );
        let c = SessionCost {
            total_usd: Some(0.001),
            ..cost(CostSource::Agent)
        };
        assert_eq!(cost_summary(&c, "Codex").total, "<$0.01");
        assert_eq!(cost_summary(&c, "Codex").notes, ["reported by Codex"]);
        let c = SessionCost {
            subagents_usd: Some(3.375),
            unpriced_models: Some(vec!["m1".into(), "m2".into()]),
            unreadable_subagents: Some(2),
            ..cost(CostSource::Estimate)
        };
        let s = cost_summary(&c, "Pi");
        assert_eq!(s.total, "—");
        assert_eq!(
            s.notes,
            [
                "subagents ~$3.38",
                "unpriced: m1, m2",
                "2 subagent(s) unreadable"
            ]
        );
    }
}
