//! Readers for the JSON the server writes: the inverse of
//! [`super::write`], for clients that talk to a remote server.

use crate::js::value::JsValue;
use crate::model::{Agent, ProjectSlug, SessionHeader, SessionId, SourceTag};
use crate::sessions::model::SessionCost;
pub use crate::sessions::read_parsed::parsed_from_json;

fn text(v: &JsValue, key: &str) -> Option<String> {
    v.get(key).and_then(JsValue::as_str).map(str::to_owned)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "counts are non-negative integers"
)]
fn count(v: &JsValue, key: &str) -> Option<u64> {
    v.get(key)
        .and_then(JsValue::as_num)
        .filter(|n| *n >= 0.0)
        .map(|n| n as u64)
}

fn texts(v: &JsValue, key: &str) -> Option<Vec<String>> {
    v.get(key)?
        .as_arr()?
        .iter()
        .map(|s| s.as_str().map(str::to_owned))
        .collect()
}

/// One header object, as [`super::write::write_header`] writes it.
pub fn header_from_json(v: &JsValue) -> Option<SessionHeader> {
    let source = match (text(v, "source"), text(v, "sourceLabel")) {
        (Some(id), Some(label)) => Some(SourceTag { id, label }),
        _ => None,
    };
    Some(SessionHeader {
        id: SessionId::new(text(v, "id")?),
        agent: Agent::parse(v.get("agent")?.as_str()?)?,
        path: text(v, "path")?,
        project: ProjectSlug::new(text(v, "project")?),
        message_count: count(v, "messageCount")?,
        size_bytes: count(v, "sizeBytes")?,
        updated_at: text(v, "updatedAt")?,
        cwd: text(v, "cwd"),
        git_branch: text(v, "gitBranch"),
        model: text(v, "model"),
        title: text(v, "title"),
        started_at: text(v, "startedAt"),
        malformed_lines: count(v, "malformedLines"),
        cost_usd: v.get("costUsd").and_then(JsValue::as_num),
        unpriced_models: texts(v, "unpricedModels"),
        source,
    })
}

/// The headers and the unreadable count of a `{headers, unreadable}`
/// listing.
pub fn listing_from_json(v: &JsValue) -> Option<(Vec<SessionHeader>, usize)> {
    let headers = v
        .get("headers")?
        .as_arr()?
        .iter()
        .map(header_from_json)
        .collect::<Option<Vec<_>>>()?;
    let unreadable = v.get("unreadable")?.as_arr()?.len();
    Some((headers, unreadable))
}

/// An analysis `cost` object.
pub fn cost_from_json(v: &JsValue) -> Option<SessionCost> {
    Some(SessionCost {
        from_agent: v.get("source")?.as_str()? == "agent",
        total_usd: v.get("totalUsd").and_then(JsValue::as_num),
        subagents_usd: v.get("subagentsUsd").and_then(JsValue::as_num),
        unpriced_models: texts(v, "unpricedModels"),
        #[expect(clippy::cast_possible_truncation, reason = "a small count")]
        unreadable_subagents: count(v, "unreadableSubagents").map(|n| n as usize),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::write::headers_json;

    #[test]
    fn header_round_trip() {
        let h = SessionHeader {
            id: SessionId::new("s1"),
            agent: Agent::Pi,
            path: "/p/s1.jsonl".to_owned(),
            project: ProjectSlug::new("--x--"),
            message_count: 3,
            size_bytes: 10,
            updated_at: "2026-01-01T00:00:00.000Z".to_owned(),
            cwd: Some("/x".to_owned()),
            git_branch: None,
            model: Some("m".to_owned()),
            title: Some("t\u{10F800}".to_owned()),
            started_at: Some("2026".to_owned()),
            malformed_lines: Some(1),
            cost_usd: Some(0.25),
            unpriced_models: Some(vec!["u".to_owned()]),
            source: Some(SourceTag {
                id: "pi:w".to_owned(),
                label: "w".to_owned(),
            }),
        };
        let json = headers_json([&h], false);
        let back = JsValue::parse(&json).and_then(|v| {
            v.as_arr()
                .and_then(|a| a.first().and_then(header_from_json))
        });
        assert_eq!(back.as_ref(), Some(&h));
        assert_eq!(headers_json(back.as_ref(), false), json);
    }
}
