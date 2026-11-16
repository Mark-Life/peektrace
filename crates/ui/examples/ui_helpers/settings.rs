//! Probe answers for the Settings, Capabilities, theme and shortcut rules.

use peektrace_protocol::{AgentId, ApiError, Capability, PeektraceSettings, ServerInfo};
use peektrace_ui::model::capabilities::{
    AGENT_COLUMNS, CAPABILITY_HEAD, LEGEND_HEAD, LEVELS, NOTE_HEAD, cell_note, group_by,
    level_gloss, level_label, level_name, support,
};
use peektrace_ui::model::settings::{
    AGENT_ORDER, Rows, assemble, copy, default_roots, read_only_body, restart_body, saved_note,
    scans, seed_rows, seg_text, tilde_path, wire_error_message,
};
use peektrace_ui::model::shortcuts::{
    GROUPS, HELP_BUTTON, HELP_NOTE, HELP_TITLE, Modifiers, SHORTCUTS, group_note, key_label,
    key_of, shortcut_allowed,
};
use peektrace_ui::model::theme_mode::{Appearance, next_theme_press};
use serde_json::{Map, Value, json};

fn rows_json(rows: &Rows) -> Value {
    AGENT_ORDER
        .iter()
        .zip(rows)
        .map(|(a, list)| {
            let list: Vec<Value> = list
                .iter()
                .map(|(p, l)| json!({ "path": p, "label": l }))
                .collect();
            (a.as_str().to_owned(), Value::Array(list))
        })
        .collect::<Map<String, Value>>()
        .into()
}

/// Edited rows as the TS side passes them: `{agent: [{path, label}]}`.
fn rows_of(v: &Value) -> Rows {
    AGENT_ORDER.map(|a| {
        v[a.as_str()]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|r| {
                        let s = |k: &str| r[k].as_str().unwrap_or_default().to_owned();
                        (s("path"), s("label"))
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn to_value<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

pub fn settings_answer(req: &Value) -> Value {
    let cases = req["cases"].as_array().map_or(&[][..], Vec::as_slice);
    Value::Array(
        cases
            .iter()
            .map(|c| {
                let settings: PeektraceSettings =
                    serde_json::from_value(c["settings"].clone()).unwrap_or_default();
                let seed = seed_rows(&settings);
                let defaults = serde_json::from_value::<ServerInfo>(c["info"].clone())
                    .map(|info| {
                        default_roots(&info)
                            .into_iter()
                            .map(|(a, r)| json!([a.as_str(), to_value(r)]))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                json!({
                    "seed": rows_json(&seed),
                    "assembled": to_value(&assemble(&seed)),
                    "assembledEdited": to_value(&assemble(&rows_of(&c["rows"]))),
                    "defaults": defaults,
                })
            })
            .collect(),
    )
}

pub fn tilde_answer(req: &Value) -> Value {
    let pairs = req["pairs"].as_array().map_or(&[][..], Vec::as_slice);
    pairs
        .iter()
        .map(|p| {
            tilde_path(
                p[0].as_str().unwrap_or_default(),
                p[1].as_str().unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>()
        .into()
}

pub fn wire_answer(req: &Value) -> Value {
    let errors = req["errors"].as_array().map_or(&[][..], Vec::as_slice);
    errors
        .iter()
        .map(|e| {
            serde_json::from_value::<ApiError>(e.clone())
                .map_or_else(|e| format!("undecodable: {e}"), |e| wire_error_message(&e))
        })
        .collect::<Vec<_>>()
        .into()
}

pub fn caps_answer(req: &Value) -> Value {
    let caps: Vec<Capability> = serde_json::from_value(req["caps"].clone()).unwrap_or_default();
    let groups: Vec<Value> = group_by(&caps)
        .into_iter()
        .map(|(g, rows)| json!([g, rows.iter().map(|c| c.id.as_str()).collect::<Vec<_>>()]))
        .collect();
    let notes: Map<String, Value> = caps
        .iter()
        .map(|c| {
            let per: Map<String, Value> = AGENT_COLUMNS
                .iter()
                .map(|(a, _)| (a.as_str().to_owned(), json!(cell_note(support(c, *a)))))
                .collect();
            (c.id.clone(), Value::Object(per))
        })
        .collect();
    json!({
        "columns": AGENT_COLUMNS.iter().map(|(a, l)| json!([a.as_str(), l])).collect::<Vec<_>>(),
        "levels": LEVELS.iter().map(|l| level_name(*l)).collect::<Vec<_>>(),
        "gloss": LEVELS.iter().map(|l| (level_name(*l).to_owned(), json!(level_gloss(*l)))).collect::<Map<_, _>>(),
        "labels": LEVELS.iter().map(|l| (level_name(*l).to_owned(), json!(level_label(*l)))).collect::<Map<_, _>>(),
        "groups": groups,
        "notes": notes,
    })
}

pub fn theme_answer(req: &Value) -> Value {
    let look = |v: &Value| v.as_str().and_then(Appearance::parse);
    let pairs = req["pairs"].as_array().map_or(&[][..], Vec::as_slice);
    pairs
        .iter()
        .map(|p| {
            let press = next_theme_press(look(&p[0]), look(&p[1]));
            json!({ "appearance": press.appearance.as_str(), "store": press.store.as_str() })
        })
        .collect::<Vec<_>>()
        .into()
}

pub fn keys_answer(req: &Value) -> Value {
    let table: Vec<Value> = SHORTCUTS
        .iter()
        .map(|s| {
            json!({ "id": s.id, "keys": s.keys, "label": s.label, "group": s.group.as_str(), "scope": s.scope.as_str() })
        })
        .collect();
    let labels = |mac: bool| -> Map<String, Value> {
        SHORTCUTS
            .iter()
            .flat_map(|s| s.keys.iter())
            .map(|k| ((*k).to_owned(), json!(key_label(k, mac))))
            .collect()
    };
    let events = req["events"].as_array().map_or(&[][..], Vec::as_slice);
    let key_ofs: Vec<String> = events
        .iter()
        .map(|e| {
            key_of(
                e[0].as_str().unwrap_or_default(),
                e[1].as_str().unwrap_or_default(),
                e[2].as_bool().unwrap_or_default(),
            )
        })
        .collect();
    let allowed = req["allowed"].as_array().map_or(&[][..], Vec::as_slice);
    let allowed: Vec<bool> = allowed
        .iter()
        .map(|a| {
            let b = |i: usize| a[i].as_bool().unwrap_or_default();
            let m = Modifiers {
                meta: b(0),
                ctrl: b(1),
                alt: b(2),
                alt_graph: b(3),
            };
            shortcut_allowed(m, a[4].as_str().unwrap_or_default(), b(5))
        })
        .collect();
    json!({
        "table": table,
        "labels": { "mac": labels(true), "other": labels(false) },
        "groups": GROUPS.iter().map(|g| g.as_str()).collect::<Vec<_>>(),
        "notes": GROUPS.iter().filter_map(|g| group_note(*g).map(|n| (g.as_str().to_owned(), json!(n)))).collect::<Map<_, _>>(),
        "keyOf": key_ofs,
        "allowed": allowed,
    })
}

/// Every copy string the views draw, and the native-only ones the
/// inspector has no twin for. `path` stands in for the settings file.
pub fn copy_answer() -> Value {
    let path = "~/.peektrace/settings.json";
    let mut web = vec![
        copy::HEADING.to_owned(),
        copy::HEADING_NOTE.to_owned(),
        copy::RESTART_TITLE.to_owned(),
        seg_text(&restart_body(path, false)),
        copy::READ_ONLY_TITLE.to_owned(),
        seg_text(&read_only_body()),
        copy::CONFLICT_TITLE.to_owned(),
        copy::CONFLICT_BODY.to_owned(),
        copy::CONFLICT_RELOAD.to_owned(),
        copy::CONFLICT_OVERWRITE.to_owned(),
        copy::AGENT_NOTE.to_owned(),
        copy::DEFAULT_LABEL.to_owned(),
        seg_text(&scans("~/.claude/projects")),
        copy::NOT_FOUND.to_owned(),
        copy::NO_ROOTS.to_owned(),
        copy::PATH_LABEL.to_owned(),
        copy::PATH_PLACEHOLDER.to_owned(),
        copy::LABEL_LABEL.to_owned(),
        copy::LABEL_PLACEHOLDER.to_owned(),
        copy::REMOVE_ROOT.to_owned(),
        copy::ADD_ROOT.to_owned(),
        copy::SAVE.to_owned(),
        copy::SAVING.to_owned(),
        copy::CAPS_TITLE.to_owned(),
        copy::CAPS_NOTE.to_owned(),
        copy::CAPS_OPEN.to_owned(),
        copy::SAVED.to_owned(),
        saved_note(false).to_owned(),
        copy::SAVE_FAILED.to_owned(),
        copy::TYPED_FAILURE.to_owned(),
        copy::UNTYPED_FAILURE.to_owned(),
        copy::RETRY.to_owned(),
        copy::RETRYING.to_owned(),
        CAPABILITY_HEAD.to_owned(),
        NOTE_HEAD.to_owned(),
        LEGEND_HEAD.to_owned(),
        HELP_TITLE.to_owned(),
        HELP_NOTE.to_owned(),
        HELP_BUTTON.to_owned(),
    ];
    web.extend(
        [
            AgentId::Claude,
            AgentId::Codex,
            AgentId::Pi,
            AgentId::Opencode,
        ]
        .map(|a| peektrace_ui::model::list::agent_label(a).to_owned()),
    );
    json!({
        "web": web,
        "native": [seg_text(&restart_body(path, true)), saved_note(true)],
    })
}
