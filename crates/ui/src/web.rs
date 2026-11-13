//! The web build keeps the open session in `location.hash`, in the
//! inspector's shape: `#/sessions?id=<id>&parent=<parent>`.

use crate::state::Selected;

/// `id` and `parent` from a hash like `#/sessions?id=X&parent=Y`.
pub fn parse_hash(hash: &str) -> Option<Selected> {
    let query = hash.trim_start_matches('#').strip_prefix("/sessions?")?;
    let mut id = None;
    let mut parent = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = decode(v);
        match k {
            "id" if !v.is_empty() => id = Some(v),
            "parent" if !v.is_empty() => parent = Some(v),
            _ => {}
        }
    }
    id.map(|id| Selected { id, parent })
}

/// The hash for a selection.
pub fn hash_of(sel: &Selected) -> String {
    let parent = sel
        .parent
        .as_deref()
        .map(|p| format!("&parent={}", encode(p)))
        .unwrap_or_default();
    format!("#/sessions?id={}{parent}", encode(&sel.id))
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b[i], hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (b'+', _) => {
                out.push(b' ');
                i += 1;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(target_arch = "wasm32")]
pub fn read() -> Option<Selected> {
    parse_hash(&web_sys::window()?.location().hash().ok()?)
}

/// Write the selection to the hash without a history entry.
#[cfg(target_arch = "wasm32")]
pub fn write(sel: &Selected) {
    let Some(w) = web_sys::window() else { return };
    let hash = hash_of(sel);
    if w.location().hash().ok().as_deref() == Some(hash.as_str()) {
        return;
    }
    if let Ok(h) = w.history() {
        let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&hash));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_round_trip() {
        let sel = Selected {
            id: "agent-a 1".into(),
            parent: Some("p/1".into()),
        };
        let h = hash_of(&sel);
        assert_eq!(h, "#/sessions?id=agent-a%201&parent=p%2F1");
        assert_eq!(parse_hash(&h), Some(sel));
        assert_eq!(parse_hash("#/stats"), None);
        assert_eq!(parse_hash("#/sessions?q=x"), None);
    }
}
