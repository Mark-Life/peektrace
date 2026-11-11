//! Pi sessions are trees linked by `parentId`. As in Pi's session manager,
//! the leaf is the last entry and the active branch is its walk to the root.

use std::collections::{HashMap, HashSet};

use crate::js::value::JsValue;

/// Row indexes on the active branch plus the `session` rows, or `None` to
/// keep every row: an entry without a string `id` or a `parentId` key, a
/// duplicate id, a missing parent or a cycle means the tree is not trusted.
/// `null` rows are skipped.
pub fn active_branch(rows: &[JsValue]) -> Option<HashSet<usize>> {
    let mut by_id: HashMap<&str, usize> = HashMap::new();
    let mut keep = HashSet::new();
    let mut last = None;
    for (index, row) in rows.iter().enumerate() {
        if row.is_null() {
            continue;
        }
        if row.get("type").and_then(JsValue::as_str) == Some("session") {
            keep.insert(index);
            continue;
        }
        let id = row.get("id").and_then(JsValue::as_str)?;
        if row.as_obj().is_none_or(|o| o.get("parentId").is_none()) || by_id.contains_key(id) {
            return None;
        }
        by_id.insert(id, index);
        last = Some(index);
    }
    let mut at = last;
    while let Some(i) = at {
        if !keep.insert(i) {
            return None;
        }
        match rows[i].get("parentId") {
            Some(JsValue::Null) => return Some(keep),
            Some(JsValue::Str(parent)) => at = by_id.get(parent.as_str()).copied(),
            _ => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(lines: &[&str]) -> Vec<JsValue> {
        lines
            .iter()
            .map(|l| JsValue::parse(l).unwrap_or(JsValue::Null))
            .collect()
    }

    fn sorted(s: Option<HashSet<usize>>) -> Option<Vec<usize>> {
        s.map(|s| {
            let mut v: Vec<_> = s.into_iter().collect();
            v.sort_unstable();
            v
        })
    }

    #[test]
    fn walks_from_the_last_entry() {
        let r = rows(&[
            r#"{"type":"session","id":"s"}"#,
            r#"{"id":"a","parentId":null}"#,
            r#"{"id":"b","parentId":"a"}"#,
            r#"{"id":"c","parentId":"a"}"#,
        ]);
        assert_eq!(sorted(active_branch(&r)), Some(vec![0, 1, 3]));
    }

    #[test]
    fn untrusted_trees_keep_everything() {
        for bad in [
            vec![r#"{"id":"a"}"#],
            vec![
                r#"{"id":"a","parentId":null}"#,
                r#"{"id":"a","parentId":"a"}"#,
            ],
            vec![r#"{"id":"a","parentId":"zz"}"#],
            vec![
                r#"{"id":"a","parentId":"b"}"#,
                r#"{"id":"b","parentId":"a"}"#,
            ],
            vec![r#"{"id":"a","parentId":5}"#],
            vec!["5", r#"{"id":"a","parentId":null}"#],
        ] {
            assert_eq!(active_branch(&rows(&bad)), None, "{bad:?}");
        }
    }
}
