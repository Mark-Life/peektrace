//! `peektrace statusline`: one-line context health for Claude Code's
//! `statusLine`. Claude Code pipes a JSON blob to stdin on every refresh;
//! this analyzes the transcript it names and prints `ctx 42% Healthy`. Any
//! failure (bad stdin, missing session, timeout) prints `ctx --` and exits 0.

use std::io::{IsTerminal as _, Read};
use std::sync::mpsc;
use std::time::Duration;

use peektrace_core::js::num;
use peektrace_core::js::value::JsValue;
use peektrace_core::sessions::service::{
    AnalyzeOverrides, SessionEnv, Target, analyze_session_with,
};
use tracing::Span;

use crate::ls::{Context, UserError, parse_limit};
use crate::remote::Remote;
use crate::single::agent_roots;
use crate::verdict::{fraction_of, verdict};
use crate::zone::SystemZone;

pub const FALLBACK: &str = "ctx --";
const MAX_STDIN_BYTES: usize = 1_000_000;
const CHUNK_BYTES: usize = 64 * 1024;
const MAX_PERCENT: f64 = 999.0;
const NOT_FOUND_TAG: &str = "SessionNotFoundError";

/// The fields of Claude Code's payload this reads; the rest is ignored.
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    /// A session id that looks like a UUID.
    pub session_id: Option<String>,
    /// A non-empty `transcript_path`.
    pub path: Option<String>,
    /// `context_window.context_window_size` when it is a number above 0.
    pub window: Option<f64>,
}

impl Input {
    /// The id or path to look up first, then the one to retry with when the
    /// first is not found.
    fn lookups(&self) -> Option<(&str, Option<&str>)> {
        match (&self.path, &self.session_id) {
            (Some(p), id) => Some((p.as_str(), id.as_deref())),
            (None, Some(id)) => Some((id.as_str(), None)),
            (None, None) => None,
        }
    }
}

/// `/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i`.
pub fn is_uuid(s: &str) -> bool {
    let groups = [8, 4, 4, 4, 12];
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == groups.len()
        && parts
            .iter()
            .zip(groups)
            .all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// A string field: absent and `null` read as `None`; any other type fails
/// the whole decode.
fn string_field(v: &JsValue, key: &str) -> Result<Option<String>, ()> {
    match v.get(key) {
        None | Some(JsValue::Null) => Ok(None),
        Some(JsValue::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(()),
    }
}

/// Decode the stdin text; `None` gives the fallback.
pub fn parse_input(text: &str) -> Option<Input> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let v = JsValue::parse(text)?;
    if !matches!(v, JsValue::Obj(_)) {
        return None;
    }
    let session_id = string_field(&v, "session_id").ok()?;
    let path = string_field(&v, "transcript_path").ok()?;
    let window = match v.get("context_window") {
        Some(w @ JsValue::Obj(_)) => w
            .get("context_window_size")
            .and_then(JsValue::as_num)
            .filter(|n| *n > 0.0),
        _ => None,
    };
    Some(Input {
        session_id: session_id.filter(|s| is_uuid(s)),
        path: path.filter(|p| !p.is_empty()),
        window,
    })
}

/// Read up to the cap; the chunk that crosses it is kept. A read error
/// reads as empty input.
pub fn read_capped(mut r: impl Read) -> String {
    let mut buf = Vec::new();
    let mut chunk = vec![0; CHUNK_BYTES];
    while buf.len() < MAX_STDIN_BYTES {
        match r.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return String::new(),
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// `ctx <N>% <Verdict>` for the final context fill.
pub fn render(final_tokens: f64, window: f64, dumb_zone: f64) -> String {
    let fill = fraction_of(final_tokens, window);
    let shown = num::js_round(fill * 100.0).min(MAX_PERCENT);
    format!(
        "ctx {}% {}",
        num::to_js_string(shown),
        verdict(fill, dumb_zone)
    )
}

/// A failed analysis: whether the session was not found.
struct Missed {
    not_found: bool,
}

fn analyze_local(ctx: &Context, id: &str, window: Option<f64>) -> Result<String, Missed> {
    let (roots, _) = agent_roots(ctx);
    let zone = SystemZone::default();
    let env = SessionEnv {
        roots: &roots,
        local: &zone,
    };
    let overrides = AnalyzeOverrides {
        window,
        dumb_zone: None,
    };
    analyze_session_with(env, Target::Lookup(id), None, true, overrides)
        .map(|a| {
            render(
                a.final_context_tokens,
                a.context_window,
                a.dumb_zone_fraction,
            )
        })
        .map_err(|e| Missed {
            not_found: matches!(
                e,
                peektrace_core::sessions::resolve::SessionError::NotFound { .. }
            ),
        })
}

fn analyze_remote(remote: &Remote, id: &str, window: Option<f64>) -> Result<String, Missed> {
    let window = window.map(num::to_js_string);
    let query: Vec<(&str, &str)> = window
        .as_deref()
        .map(|w| ("window", w))
        .into_iter()
        .collect();
    let analysis = remote
        .get(&["sessions", id, "analysis"], &query)
        .map_err(|f| Missed {
            not_found: f.tag.as_deref() == Some(NOT_FOUND_TAG),
        })?;
    let num_of = |k: &str| analysis.get(k).and_then(JsValue::as_num);
    match (
        num_of("finalContextTokens"),
        num_of("contextWindow"),
        num_of("dumbZoneFraction"),
    ) {
        (Some(tokens), Some(size), Some(dumb)) => Ok(render(tokens, size, dumb)),
        _ => Err(Missed { not_found: false }),
    }
}

/// The line for `stdin`, or the fallback.
fn line_for(text: &str, ctx: &Context, remote: Option<&Remote>) -> String {
    let Some(input) = parse_input(text) else {
        return FALLBACK.to_owned();
    };
    let Some((first, retry)) = input.lookups() else {
        return FALLBACK.to_owned();
    };
    let analyze = |id: &str| match remote {
        Some(r) => analyze_remote(r, id, input.window),
        None => analyze_local(ctx, id, input.window),
    };
    match analyze(first) {
        Ok(line) => line,
        Err(Missed { not_found: true }) => retry
            .and_then(|id| analyze(id).ok())
            .unwrap_or_else(|| FALLBACK.to_owned()),
        Err(_) => FALLBACK.to_owned(),
    }
}

/// `--timeout`, validated as `@effect/cli` reads an integer.
pub fn parse_timeout(raw: &str) -> Result<Duration, UserError> {
    let ms = parse_limit(raw)?.max(1.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a safe integer of at least 1"
    )]
    Ok(Duration::from_millis(ms as u64))
}

/// Read stdin and analyze on a worker thread; the fallback when it misses
/// `deadline`. A thread stuck on a held-open stdin is left behind: the
/// process exits without joining it.
pub fn run(ctx: Context, remote: Option<Remote>, deadline: Duration) -> String {
    let (tx, rx) = mpsc::channel();
    let parent = Span::current();
    let spawned = std::thread::Builder::new()
        .name("statusline".to_owned())
        .spawn(move || {
            let _entered = parent.enter();
            let stdin = std::io::stdin();
            let text = if stdin.is_terminal() {
                String::new()
            } else {
                read_capped(stdin.lock())
            };
            let _ = tx.send(line_for(&text, &ctx, remote.as_ref()));
        });
    if spawned.is_err() {
        return FALLBACK.to_owned();
    }
    rx.recv_timeout(deadline)
        .unwrap_or_else(|_| FALLBACK.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "44444444-4444-4444-8444-444444444444";

    #[test]
    fn uuids() {
        assert!(is_uuid(ID));
        assert!(is_uuid(&ID.to_uppercase()));
        assert!(!is_uuid("44444444"));
        assert!(!is_uuid("4444444g-4444-4444-8444-444444444444"));
        assert!(!is_uuid(&format!("{ID}-1")));
    }

    #[test]
    fn inputs() {
        let p = |s: &str| parse_input(s);
        assert_eq!(
            p(&format!(r#"{{"session_id":"{ID}"}}"#)).and_then(|i| i.session_id),
            Some(ID.to_owned())
        );
        assert_eq!(
            p(r#"{"session_id":"4444"}"#).map(|i| i.session_id),
            Some(None)
        );
        assert_eq!(p(r#"{"session_id":5}"#), None);
        assert_eq!(p(r#"{"transcript_path":5}"#), None);
        assert_eq!(p("[]"), None);
        assert_eq!(p("null"), None);
        assert_eq!(p("not json"), None);
        assert_eq!(p(""), None);
        assert_eq!(p("{} x"), None);
        let none = p(r#"{"session_id":null,"transcript_path":null}"#);
        assert_eq!(none.and_then(|i| i.lookups().map(|_| ())), None);
        assert_eq!(p(r#"{"transcript_path":""}"#).and_then(|i| i.path), None);
        assert_eq!(
            p("\u{feff}  {\"transcript_path\":\"/a\"}\n").and_then(|i| i.path),
            Some("/a".to_owned())
        );
        let w = |s: &str| p(&format!(r#"{{"context_window":{s}}}"#)).and_then(|i| i.window);
        assert_eq!(w(r#"{"context_window_size":200000}"#), Some(200_000.0));
        assert_eq!(w(r#"{"context_window_size":1000.5}"#), Some(1000.5));
        assert_eq!(w(r#"{"context_window_size":0}"#), None);
        assert_eq!(w(r#"{"context_window_size":-5}"#), None);
        assert_eq!(w(r#"{"context_window_size":"200000"}"#), None);
        assert_eq!(w("null"), None);
        assert_eq!(
            p(r#"{"transcript_path":"/a","transcript_path":"/b"}"#).and_then(|i| i.path),
            Some("/b".to_owned())
        );
    }

    #[test]
    fn retry_order() {
        let i = Input {
            session_id: Some(ID.to_owned()),
            path: Some("/p".to_owned()),
            window: None,
        };
        assert_eq!(i.lookups(), Some(("/p", Some(ID))));
        let i = Input { path: None, ..i };
        assert_eq!(i.lookups(), Some((ID, None)));
    }

    #[test]
    fn lines() {
        assert_eq!(render(50_000.0, 1_000_000.0, 0.4), "ctx 5% Healthy");
        assert_eq!(render(150_000.0, 200_000.0, 0.4), "ctx 75% Rotting");
        assert_eq!(render(150_000.0, 300_000.0, 0.4), "ctx 50% Degrading");
        assert_eq!(render(150_000.0, 1000.0, 0.4), "ctx 999% Rotting");
        assert_eq!(render(0.0, 1_000_000.0, 0.4), "ctx 0% Healthy");
        assert_eq!(render(5.0, 0.0, 0.4), "ctx 0% Healthy");
    }

    #[test]
    fn capped_read_keeps_the_crossing_chunk() {
        let big = "a".repeat(MAX_STDIN_BYTES + 3 * CHUNK_BYTES);
        let text = read_capped(big.as_bytes());
        assert!(text.len() >= MAX_STDIN_BYTES && text.len() < MAX_STDIN_BYTES + CHUNK_BYTES);
        assert_eq!(read_capped(&b"ab\xffc"[..]), "ab\u{fffd}c");
    }

    #[test]
    fn timeouts() {
        assert_eq!(parse_timeout("0"), Ok(Duration::from_millis(1)));
        assert_eq!(parse_timeout("-5"), Ok(Duration::from_millis(1)));
        assert_eq!(parse_timeout("1e3"), Ok(Duration::from_millis(1000)));
        assert_eq!(parse_timeout("0x10"), Ok(Duration::from_millis(16)));
        assert!(parse_timeout("1.5").is_err());
    }
}
