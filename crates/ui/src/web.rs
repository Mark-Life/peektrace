//! The web build keeps the open session in `location.hash`, in the
//! inspector's shape: `#/sessions?id=<id>&parent=<parent>`, and shows
//! times in the browser's zone.

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

/// The browser's zone, as `Date` reads local fields.
#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone, Copy, Default)]
pub struct BrowserZone;

#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "Date time values are whole ms within ±8.64e15"
)]
impl peektrace_js::time::LocalZone for BrowserZone {
    fn offset_at_utc_ms(&self, utc_ms: i64) -> i64 {
        let minutes = js_sys::Date::new(&(utc_ms as f64).into()).get_timezone_offset();
        -(minutes * 60_000.0) as i64
    }

    /// `new Date(y, m, d, h, mi, s, ms)` on the wall fields resolves skipped
    /// and repeated times as `Date.parse` does.
    fn offset_at_wall_ms(&self, wall_ms: i64) -> i64 {
        let w = js_sys::Date::new(&(wall_ms as f64).into());
        let i = |v: u32| i32::try_from(v).unwrap_or(0);
        let local = js_sys::Date::new_with_year_month_day_hr_min_sec_milli(
            w.get_utc_full_year(),
            i(w.get_utc_month()),
            i(w.get_utc_date()),
            i(w.get_utc_hours()),
            i(w.get_utc_minutes()),
            i(w.get_utc_seconds()),
            i(w.get_utc_milliseconds()),
        );
        wall_ms - local.get_time() as i64
    }
}
