//! The list header of one `OpenCode` session.

use super::parse::{agent_cost, billed, iso_from_ms, model_id, num, positive, turn_for_message};
use super::session::Session;
use crate::js::time;
use crate::js::value::str_of;
use crate::model::{Agent, ProjectSlug, SessionFileRef, SessionHeader};
use crate::sessions::cost::turns_cost;
use crate::sessions::model::TurnsCost;

/// Last non-empty segment, split on `/` and `\`.
fn basename(p: &str) -> &str {
    p.split(['/', '\\']).rfind(|s| !s.is_empty()).unwrap_or("")
}

/// `new Date(ms).toISOString()`. TS throws on a time it cannot hold; such a
/// time prints as the epoch here.
fn iso_any(ms: f64) -> String {
    #[expect(clippy::cast_possible_truncation, reason = "bounded above")]
    let ms = if ms.is_finite() && ms.abs() <= 8.64e15 {
        ms.trunc() as i64
    } else {
        0
    };
    time::to_iso_string(ms)
}

/// Header for a session listed as `r`.
pub(super) fn build_header(session: &Session, r: &SessionFileRef) -> SessionHeader {
    let info = &session.info;
    let model = model_id(info.model.as_ref());
    let (turns, message_costs): (Vec<_>, Vec<_>) = session
        .messages
        .iter()
        .filter(|m| str_of(m.data.get("role")) == "assistant")
        .map(|m| {
            let t = turn_for_message(&m.data, None, model.as_deref(), 0.0);
            let cost = billed(&t).then(|| positive(num(m.data.get("cost"))));
            (t, cost)
        })
        .unzip();
    let message_costs: Vec<_> = message_costs.into_iter().flatten().collect();
    let cost = agent_cost(positive(info.cost), &message_costs).map_or_else(
        || turns_cost(&turns),
        |own| TurnsCost {
            cost_usd: Some(own),
            unpriced_models: None,
        },
    );
    let cwd = info.directory.as_deref();
    let project = [cwd.map_or("", basename), r.slug.as_str(), r.id.as_str()]
        .into_iter()
        .find(|p| !p.is_empty())
        .unwrap_or_default();
    SessionHeader {
        id: r.id.clone(),
        agent: Agent::Opencode,
        path: r.path.clone(),
        project: ProjectSlug::new(project),
        message_count: u64::try_from(session.messages.len()).unwrap_or(u64::MAX),
        size_bytes: session.dialect_len(),
        updated_at: iso_from_ms(info.time_updated).unwrap_or_else(|| iso_any(info.time_updated)),
        cwd: info.directory.clone(),
        git_branch: None,
        model,
        title: info.title.clone(),
        started_at: iso_from_ms(info.time_created),
        malformed_lines: None,
        cost_usd: cost.cost_usd,
        unpriced_models: cost.unpriced_models,
        source: r.source.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basenames_and_dates() {
        assert_eq!(basename("C:\\Users\\x\\proj"), "proj");
        assert_eq!(basename("/a/b/"), "b");
        assert_eq!(basename("/"), "");
        assert_eq!(iso_any(-5.5), "1969-12-31T23:59:59.995Z");
        assert_eq!(iso_any(9e15), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            iso_from_ms(2000.75).as_deref(),
            Some("1970-01-01T00:00:02.000Z")
        );
    }
}
