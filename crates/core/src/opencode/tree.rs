//! `OpenCode`'s legacy JSON tree under `<data>/storage/`: one file per
//! session, message and part.

use std::collections::HashMap;
use std::fs;

use super::db::entries;
use super::session::{Cell, Message, Session, SessionInfo, finish_message, hydrate_part};
use crate::js::text;
use crate::js::value::{JsObject, JsValue};
use crate::paths::PathStyle;

const STYLE: PathStyle = PathStyle::NATIVE;
const JSON_EXT: &str = ".json";

/// A file's text parsed to a plain object.
fn read_object(path: &str) -> Option<JsObject> {
    let bytes = fs::read(path).ok()?;
    match JsValue::parse(&String::from_utf8_lossy(&bytes))? {
        JsValue::Obj(o) => Some(o),
        _ => None,
    }
}

/// Subdirectories of `dir` (after symlinks), in directory order.
fn subdirs(dir: &str) -> Vec<String> {
    entries(dir)
        .into_iter()
        .filter(|n| fs::metadata(STYLE.join(&[dir, n])).is_ok_and(|m| m.is_dir()))
        .collect()
}

/// `*.json` names in `dir`, in directory order.
fn json_names(dir: &str) -> Vec<String> {
    entries(dir)
        .into_iter()
        .filter(|n| n.ends_with(JSON_EXT))
        .collect()
}

/// `*.json` names sorted as JS sorts strings.
fn sorted_json_names(dir: &str) -> Vec<String> {
    let mut names = json_names(dir);
    names.sort_by(|a, b| text::cmp_utf16(a, b));
    names
}

fn session_root(data_dir: &str) -> String {
    STYLE.join(&[data_dir, "storage", "session"])
}

/// A JSON value as a column would hand it over.
fn cell(v: Option<&JsValue>) -> Cell {
    match v {
        Some(JsValue::Num(n)) => Cell::Num(*n),
        Some(JsValue::Str(s)) => Cell::Text(s.clone()),
        Some(_) => Cell::Other,
        None => Cell::Null,
    }
}

/// `info.time[key]` as a number, else 0.
fn tree_time(info: &JsObject, key: &str) -> f64 {
    cell(
        info.get("time")
            .and_then(JsValue::as_obj)
            .and_then(|t| t.get(key)),
    )
    .num()
}

/// `time.updated`, else `time.created`.
fn updated(info: &JsObject) -> f64 {
    let u = tree_time(info, "updated");
    if u == 0.0 || u.is_nan() {
        tree_time(info, "created")
    } else {
        u
    }
}

/// A session id and its directory, as the tree lists it.
pub(super) struct TreeRef {
    pub id: String,
    pub directory: Option<String>,
}

/// Every session file in the tree that holds an object with a string `id`.
pub(super) fn list_tree_refs(data_dir: &str) -> Vec<TreeRef> {
    let root = session_root(data_dir);
    subdirs(&root)
        .into_iter()
        .flat_map(|proj| {
            let dir = STYLE.join(&[&root, &proj]);
            json_names(&dir)
                .into_iter()
                .filter_map(move |name| read_object(&STYLE.join(&[&dir, &name])))
        })
        .filter_map(|info| {
            let id = info.get("id")?.as_str()?.to_owned();
            Some(TreeRef {
                id,
                directory: cell(info.get("directory")).opt_str(),
            })
        })
        .collect()
}

fn stem(name: &str) -> &str {
    name.strip_suffix(JSON_EXT).unwrap_or(name)
}

fn load_parts(data_dir: &str, id: &str, message_id: &str) -> Vec<JsObject> {
    let dir = STYLE.join(&[data_dir, "storage", "part", message_id]);
    sorted_json_names(&dir)
        .iter()
        .map(|name| {
            let data = read_object(&STYLE.join(&[&dir, name]));
            hydrate_part(data, JsValue::Str(stem(name).to_owned()), id, message_id)
        })
        .collect()
}

/// Session file paths of one tree by file stem, each list in the order a
/// project-dir scan meets them. Built once per data dir so loading many
/// sessions does not rescan every project dir per session.
#[derive(Debug, Default)]
pub(super) struct TreeIndex(HashMap<String, Vec<String>>);

impl TreeIndex {
    pub(super) fn build(data_dir: &str) -> Self {
        let root = session_root(data_dir);
        Self(
            subdirs(&root)
                .iter()
                .flat_map(|proj| {
                    let dir = STYLE.join(&[&root, proj]);
                    json_names(&dir).into_iter().map(move |name| {
                        let path = STYLE.join(&[&dir, &name]);
                        (stem(&name).to_owned(), path)
                    })
                })
                .fold(HashMap::new(), |mut by_stem, (stem, path)| {
                    by_stem.entry(stem).or_insert_with(Vec::new).push(path);
                    by_stem
                }),
        )
    }

    fn paths(&self, id: &str) -> &[String] {
        self.0.get(id).map_or(&[], Vec::as_slice)
    }
}

/// The first of `paths` that holds an object with this `id`.
fn find_info(paths: impl IntoIterator<Item = String>, id: &str) -> Option<JsObject> {
    paths.into_iter().find_map(|path| {
        read_object(&path).filter(|o| o.get("id").and_then(JsValue::as_str) == Some(id))
    })
}

/// The session file for `id`, probing `<proj>/<id>.json` in every project dir.
fn scan_info(data_dir: &str, id: &str) -> Option<JsObject> {
    let root = session_root(data_dir);
    let file = format!("{id}{JSON_EXT}");
    let paths = subdirs(&root)
        .into_iter()
        .map(|proj| STYLE.join(&[&root, &proj, &file]));
    find_info(paths, id)
}

/// The session from the tree, when a session file named after `id` holds
/// that id. `index` answers exact names; the probe scan still runs on a miss,
/// since a case-insensitive disk also matches other spellings.
pub(super) fn load_from_tree(
    data_dir: &str,
    id: &str,
    index: Option<&TreeIndex>,
) -> Option<Session> {
    let info = index
        .and_then(|ix| find_info(ix.paths(id).iter().cloned(), id))
        .or_else(|| scan_info(data_dir, id))?;
    let message_dir = STYLE.join(&[data_dir, "storage", "message", id]);
    let messages = sorted_json_names(&message_dir)
        .iter()
        .filter_map(|name| {
            let data = read_object(&STYLE.join(&[&message_dir, name]))?;
            let message_id = stem(name);
            Some(Message {
                data: finish_message(data, message_id, id),
                parts: load_parts(data_dir, id, message_id),
            })
        })
        .collect();
    let opt = |k: &str| cell(info.get(k)).opt_str();
    Some(Session {
        info: SessionInfo {
            id: id.to_owned(),
            time_created: tree_time(&info, "created"),
            time_updated: updated(&info),
            cost: cell(info.get("cost")).num(),
            directory: opt("directory"),
            title: opt("title"),
            version: opt("version"),
            model: info.get("model").cloned(),
            ..SessionInfo::default()
        },
        messages,
    })
}

#[cfg(test)]
mod tests {
    use super::super::session::text_cell;
    use super::*;

    #[test]
    fn stems_and_cells() {
        assert_eq!(stem("msg_a.json"), "msg_a");
        assert_eq!(cell(Some(&JsValue::Bool(true))).num(), 0.0);
        assert_eq!(text_cell("x").opt_str().as_deref(), Some("x"));
    }
}
