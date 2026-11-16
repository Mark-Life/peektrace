//! Rows on branches a rewind abandoned. Claude Code transcripts are trees
//! linked by `parentUuid`; a rewind makes the next prompt a sibling of the
//! rewound one, so every prompt but the last under one parent starts a dead
//! branch.

use std::collections::{HashMap, HashSet};

use crate::js::value::JsValue;

fn str_field<'a>(o: &'a JsValue, key: &str) -> Option<&'a str> {
    o.get(key).and_then(JsValue::as_str)
}

fn is_true(o: &JsValue, key: &str) -> bool {
    o.get(key).is_some_and(JsValue::is_true)
}

fn has_tool_result(o: &JsValue) -> bool {
    o.get("message")
        .and_then(|m| m.get("content"))
        .and_then(JsValue::as_arr)
        .is_some_and(|c| {
            c.iter()
                .any(|b| b.get("type").and_then(JsValue::as_str) == Some("tool_result"))
        })
}

/// A prompt the user typed: tool results, meta and summaries are not.
fn is_prompt(o: &JsValue) -> bool {
    str_field(o, "type") == Some("user")
        && str_field(o, "uuid").is_some()
        && str_field(o, "parentUuid").is_some()
        && !is_true(o, "isSidechain")
        && !is_true(o, "isMeta")
        && !is_true(o, "isCompactSummary")
        && o.get("sourceToolAssistantUUID").is_none()
        && o.get("toolUseResult").is_none()
        && !has_tool_result(o)
}

/// Uuids of prompts sharing a parent, distinct in file order, all but the
/// last of each group.
fn fork_roots(rows: &[JsValue]) -> Vec<&str> {
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for o in rows.iter().filter(|o| is_prompt(o)) {
        let (Some(parent), Some(uuid)) = (str_field(o, "parentUuid"), str_field(o, "uuid")) else {
            continue;
        };
        let at = groups
            .iter()
            .position(|(p, _)| *p == parent)
            .unwrap_or_else(|| {
                groups.push((parent, Vec::new()));
                groups.len() - 1
            });
        let group = &mut groups[at].1;
        if !group.contains(&uuid) {
            group.push(uuid);
        }
    }
    groups
        .into_iter()
        .flat_map(|(_, g)| {
            let keep = g.len().saturating_sub(1);
            g.into_iter().take(keep)
        })
        .collect()
}

fn last_main_turn_row(rows: &[JsValue]) -> Option<usize> {
    rows.iter().rposition(|o| {
        matches!(str_field(o, "type"), Some("user" | "assistant"))
            && !o.get("isSidechain").is_some_and(JsValue::truthy)
    })
}

/// Row indexes on abandoned branches. Empty means keep every row: no fork,
/// or the live leaf came out dead so the tree is not trusted.
pub fn dead_branch_rows(rows: &[JsValue]) -> HashSet<usize> {
    let roots = fork_roots(rows);
    if roots.is_empty() {
        return HashSet::new();
    }
    let mut children: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut by_uuid: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, o) in rows.iter().enumerate() {
        let Some(uuid) = str_field(o, "uuid") else {
            continue;
        };
        if is_true(o, "isSidechain") {
            continue;
        }
        for parent in [
            str_field(o, "parentUuid"),
            str_field(o, "logicalParentUuid"),
        ]
        .into_iter()
        .flatten()
        {
            children.entry(parent).or_default().push(i);
        }
        by_uuid.entry(uuid).or_default().push(i);
    }
    let mut dead = HashSet::new();
    let mut visited = HashSet::new();
    let mut stack = roots;
    while let Some(uuid) = stack.pop() {
        if !visited.insert(uuid) {
            continue;
        }
        dead.extend(by_uuid.get(uuid).into_iter().flatten().copied());
        for &i in children.get(uuid).into_iter().flatten() {
            dead.insert(i);
            if let Some(child) = str_field(&rows[i], "uuid") {
                stack.push(child);
            }
        }
    }
    match last_main_turn_row(rows) {
        Some(last) if dead.contains(&last) => HashSet::new(),
        _ => dead,
    }
}
