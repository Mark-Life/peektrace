//! The Pi list header: a scan of rows without building a timeline. Cost
//! folds usage exactly as the full parse does, so the two agree.

use std::collections::HashSet;

use super::context::string_or;
use crate::js::text;
use crate::js::time;
use crate::js::value::{EMPTY_OBJECT, JsValue, defined};
use crate::model::{Agent, SessionFileRef, SessionHeader};
use crate::sessions::cost::turns_cost;
use crate::sessions::jsonl::scan_rows;
use crate::sessions::model::{Turn, TurnTokens};

/// Token counts one Pi call's `usage` reports; a missing count reads 0.
pub fn pi_tokens(usage: &JsValue) -> TurnTokens {
    let n = |k: &str| usage.get(k).and_then(JsValue::as_num).unwrap_or(0.0);
    let (input, cache_read, cache_creation) = (n("input"), n("cacheRead"), n("cacheWrite"));
    TurnTokens {
        context: input + cache_read + cache_creation,
        input,
        cache_read,
        cache_creation,
        output: n("output"),
    }
}

#[derive(Default)]
struct Acc {
    cwd: Option<String>,
    model: Option<String>,
    started_at: Option<String>,
    title: Option<String>,
    updated_at: Option<String>,
    messages: u64,
    keys: HashSet<String>,
    turns: Vec<Turn>,
}

fn str_field<'a>(v: &'a JsValue, key: &str) -> Option<&'a str> {
    v.get(key).and_then(JsValue::as_str)
}

impl Acc {
    fn apply(&mut self, row: &JsValue, index: usize) {
        if let Some(ts) = str_field(row, "timestamp").filter(|t| !t.is_empty()) {
            self.started_at.get_or_insert_with(|| ts.to_owned());
            self.updated_at = Some(ts.to_owned());
        }
        let msg = defined(row.get("message")).unwrap_or(&EMPTY_OBJECT);
        let role = str_field(msg, "role");
        let wants_model = self.model.as_deref().is_none_or(str::is_empty);
        match string_or(row.get("type"), "").as_str() {
            "session" => {
                if let Some(cwd) = str_field(row, "cwd") {
                    self.cwd = Some(cwd.to_owned());
                }
            }
            "session_info" => {
                if let Some(name) = str_field(row, "name").filter(|n| !text::trim(n).is_empty()) {
                    self.title = Some(name.to_owned());
                }
            }
            "model_change" if wants_model => {
                if let Some(m) = str_field(row, "modelId") {
                    self.model = Some(m.to_owned());
                }
            }
            "message" if wants_model && role == Some("assistant") => {
                if let Some(m) = str_field(msg, "model") {
                    self.model = Some(m.to_owned());
                }
            }
            _ => {}
        }
        if str_field(row, "type") != Some("message") {
            return;
        }
        if matches!(role, Some("user" | "assistant")) {
            self.messages += 1;
        }
        if role == Some("assistant") {
            let key = str_field(row, "id").map_or_else(|| format!("line-{index}"), str::to_owned);
            let tokens = pi_tokens(defined(msg.get("usage")).unwrap_or(&EMPTY_OBJECT));
            if tokens.context > 0.0 && self.keys.insert(key) {
                let model = string_or(msg.get("model"), "unknown");
                self.turns
                    .push(Turn::new(String::new(), model, tokens, None));
            }
        }
    }
}

/// Build the list header from a Pi session's text. `title` sits after
/// `startedAt`.
pub fn build_header(text_in: &str, r: &SessionFileRef) -> SessionHeader {
    let (rows, malformed) = scan_rows(text_in);
    let mut acc = Acc::default();
    for (index, row) in rows.iter().enumerate() {
        if row.as_obj().is_some() {
            acc.apply(row, index);
        }
    }
    let cost = turns_cost(&acc.turns);
    SessionHeader {
        id: r.id.clone(),
        agent: Agent::Pi,
        path: r.path.clone(),
        project: r.slug.clone(),
        message_count: acc.messages,
        size_bytes: r.stamp.size_bytes,
        updated_at: acc
            .updated_at
            .unwrap_or_else(|| time::to_iso_string(r.stamp.mtime_ms)),
        cwd: acc.cwd,
        git_branch: None,
        model: acc.model,
        title: acc.title,
        started_at: acc.started_at,
        malformed_lines: (malformed > 0).then_some(malformed),
        cost_usd: cost.cost_usd,
        unpriced_models: cost.unpriced_models,
        source: None,
    }
}
