//! The Codex list header: a scan of rollout rows without building a
//! timeline. The project is the cwd's last segment, since Codex has no
//! per-project dir.

use super::context::{is_injected_user_message, plain};
use crate::headers::{RowFold, fold_text};
use crate::js::time;
use crate::js::value::{JsValue, defined, js_string};
use crate::model::{Agent, ProjectSlug, SessionFileRef, SessionHeader};

#[derive(Default)]
pub(crate) struct Acc {
    messages: u64,
    cwd: Option<String>,
    git_branch: Option<String>,
    model: Option<String>,
    started_at: Option<String>,
    updated_at: Option<String>,
}

fn str_field<'a>(v: &'a JsValue, key: &str) -> Option<&'a str> {
    v.get(key).and_then(JsValue::as_str)
}

impl Acc {
    fn apply(&mut self, row: &JsValue) {
        if let Some(ts) = str_field(row, "timestamp").filter(|t| !t.is_empty()) {
            self.started_at.get_or_insert_with(|| ts.to_owned());
            self.updated_at = Some(ts.to_owned());
        }
        let Some(payload) = plain(row.get("payload")) else {
            return;
        };
        let ty = defined(row.get("type")).map(|t| js_string(Some(t)));
        let cwd = str_field(payload, "cwd");
        match ty.as_deref() {
            Some("session_meta") => {
                if let Some(cwd) = cwd {
                    self.cwd.get_or_insert_with(|| cwd.to_owned());
                }
                if let Some(b) = plain(payload.get("git")).and_then(|g| str_field(g, "branch")) {
                    self.git_branch.get_or_insert_with(|| b.to_owned());
                }
            }
            Some("turn_context") => {
                if let Some(cwd) = cwd {
                    self.cwd.get_or_insert_with(|| cwd.to_owned());
                }
                if self.model.as_deref().is_none_or(str::is_empty)
                    && let Some(m) = str_field(payload, "model")
                {
                    self.model = Some(m.to_owned());
                }
            }
            _ => {}
        }
    }
}

/// A user or assistant `message` item; developer items and user items made
/// only of injected parts do not count.
fn is_message(row: &JsValue) -> bool {
    let Some(payload) = plain(row.get("payload")) else {
        return false;
    };
    if str_field(row, "type") != Some("response_item")
        || str_field(payload, "type") != Some("message")
    {
        return false;
    }
    match str_field(payload, "role") {
        Some("assistant") => true,
        Some("user") => !is_injected_user_message(payload),
        _ => false,
    }
}

/// `path.split("/").filter(Boolean).pop()`.
fn last_segment(path: &str) -> &str {
    path.split('/').rfind(|s| !s.is_empty()).unwrap_or("")
}

impl RowFold for Acc {
    fn row(&mut self, row: JsValue) {
        if row.as_obj().is_none() {
            return;
        }
        self.apply(&row);
        if is_message(&row) {
            self.messages += 1;
        }
    }

    /// `cwd`, `gitBranch` and `model` print even when empty.
    fn header(self, r: &SessionFileRef, malformed: u64) -> SessionHeader {
        let project = [
            self.cwd.as_deref().map_or("", last_segment),
            r.slug.as_str(),
        ]
        .into_iter()
        .find(|p| !p.is_empty())
        .unwrap_or(r.id.as_str());
        SessionHeader {
            id: r.id.clone(),
            agent: Agent::Codex,
            path: r.path.clone(),
            project: ProjectSlug::new(project),
            message_count: self.messages,
            size_bytes: r.stamp.size_bytes,
            updated_at: self
                .updated_at
                .unwrap_or_else(|| time::to_iso_string(r.stamp.mtime_ms)),
            cwd: self.cwd,
            git_branch: self.git_branch,
            model: self.model,
            title: None,
            started_at: self.started_at,
            malformed_lines: (malformed > 0).then_some(malformed),
            cost_usd: None,
            unpriced_models: None,
            source: None,
        }
    }
}

/// Build the list header from a rollout's text.
pub fn build_header(text: &str, r: &SessionFileRef) -> SessionHeader {
    fold_text::<Acc>(text, r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FileStamp, SessionId};

    fn file(id: &str) -> SessionFileRef {
        SessionFileRef {
            id: SessionId::new(id),
            slug: ProjectSlug::new(""),
            path: format!("/r/{id}.jsonl"),
            stamp: FileStamp {
                size_bytes: 1,
                mtime_ms: 0,
            },
            source: None,
        }
    }

    #[test]
    fn counts_typed_users_and_assistants() {
        let text = [
            r#"{"timestamp":"t1","type":"session_meta","payload":{"cwd":"","git":{"branch":"a"}}}"#,
            r#"{"type":"session_meta","payload":{"cwd":"/x","git":{"branch":"b"}}}"#,
            r#"{"type":"turn_context","payload":{"model":""}}"#,
            r#"{"type":"turn_context","payload":{"model":"gpt-5"}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"<environment_context>x"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"<skill>s"},{"text":"do it"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"text":"x"}]}}"#,
            r#"{"timestamp":"t2","type":"response_item","payload":{"type":"message","role":"assistant"}}"#,
            "null",
            "bad",
            "{",
        ]
        .join("\n");
        let h = build_header(&text, &file("abc"));
        assert_eq!(h.message_count, 3);
        assert_eq!(h.cwd.as_deref(), Some(""));
        assert_eq!(h.project.as_str(), "abc");
        assert_eq!(h.git_branch.as_deref(), Some("a"));
        assert_eq!(h.model.as_deref(), Some("gpt-5"));
        assert_eq!(
            (h.started_at.as_deref(), h.updated_at.as_str()),
            (Some("t1"), "t2")
        );
        assert_eq!(h.malformed_lines, Some(1));
        assert_eq!(last_segment("/a/b//"), "b");
    }
}
