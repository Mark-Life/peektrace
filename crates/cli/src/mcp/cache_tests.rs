//! The server keeps list headers between calls, as the TS sessions layer
//! does: a file with the same size and mtime is not parsed again.

use std::fs;
use std::path::Path;

use peektrace_core::js::value::JsValue;
use peektrace_core::roots::RootsEnv;

use super::run_call;
use super::tools::{Shared, find};
use crate::ls::Context;

fn transcript(cwd: &str) -> String {
    format!(
        "{{\"type\":\"user\",\"sessionId\":\"s1\",\"cwd\":\"{cwd}\",\"timestamp\":\"2026-01-01T00:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"hi\"}}}}\n"
    )
}

fn shared(home: &Path) -> Shared {
    Shared::new(Context {
        roots_env: RootsEnv {
            home: home.to_string_lossy().into_owned(),
            ..Default::default()
        },
        settings: None,
        cwd: None,
        now_ms: 0.0,
    })
}

fn call(s: &Shared, tool: &str, args: &str) -> String {
    let args = JsValue::parse(args).unwrap_or(JsValue::Null);
    find(tool)
        .ok_or_else(|| format!("no tool {tool}"))
        .and_then(|t| run_call(t, &args, s))
        .map_or_else(|e| e, |a| a.text)
}

#[test]
fn list_and_search_share_one_header_cache() {
    let home = std::env::temp_dir().join(format!("peektrace-mcp-cache-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let file = home.join(".claude/projects/-proj/s1.jsonl");
    fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    fs::write(&file, transcript("/a")).expect("write");
    let mtime = fs::metadata(&file)
        .and_then(|m| m.modified())
        .expect("mtime");
    let s = shared(&home);
    let list = r#"{"format":"json","limit":0}"#;

    assert!(call(&s, "sessions_search", r#"{"q":"hi"}"#).contains("s1"));
    // Same size, same mtime: the header the search listed stands.
    fs::write(&file, transcript("/b")).expect("rewrite");
    fs::File::options()
        .write(true)
        .open(&file)
        .and_then(|f| f.set_modified(mtime))
        .expect("set mtime");
    let cached = call(&s, "sessions_list", list);
    let fresh = call(&shared(&home), "sessions_list", list);
    let _ = fs::remove_dir_all(&home);

    assert!(cached.contains(r#""cwd": "/a""#), "{cached}");
    assert!(fresh.contains(r#""cwd": "/b""#), "{fresh}");
}
