//! A synthetic HOME for the app's tests and goldens: the core fixtures
//! under the agent roots plus generated Claude sessions. Never the real
//! home. `NOW` is the fixed clock every run uses (`PEEKTRACE_APP_NOW`).

#![allow(dead_code, reason = "each test binary uses part of it")]
#![allow(clippy::format_push_string, reason = "test data")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

pub const NOW: &str = "2026-06-01T10:03:00Z";
pub const LONG_ID: &str = "33333333-3333-4333-8333-333333333333";
pub const UNTITLED_ID: &str = "44444444-4444-4444-8444-444444444444";
pub const INTERLEAVED_ID: &str = "55555555-5555-4555-8555-555555555555";
pub const SURROGATE_ID: &str = "66666666-6666-4666-8666-666666666666";
pub const FIXTURE_ID: &str = "11111111-1111-4111-8111-111111111111";
/// The long session's big tool result, in bytes.
pub const BIG_BYTES: usize = 200_000;
/// The turn whose result is big.
pub const BIG_TURN: u64 = 30;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/core/test/fixtures")
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("mkdir");
    for e in fs::read_dir(from).expect("read dir") {
        let e = e.expect("entry");
        let dst = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &dst);
        } else {
            fs::copy(e.path(), dst).expect("copy");
        }
    }
}

pub fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

/// A fresh empty HOME under the target dir.
pub fn temp_home(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "ui-home-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir home");
    dir
}

/// The env a run gets: this HOME, UTC, the fixed clock, nothing inherited.
pub fn env(home: &Path) -> Vec<(String, String)> {
    let home = home.to_string_lossy().into_owned();
    vec![
        ("HOME".into(), home.clone()),
        ("TZ".into(), "UTC".into()),
        ("PEEKTRACE_DIR".into(), format!("{home}/.peektrace")),
        ("PEEKTRACE_NO_TELEMETRY".into(), "1".into()),
        ("PEEKTRACE_APP_NOW".into(), NOW.into()),
    ]
}

/// `2026-05-31T08:00:00Z` plus `sec`.
fn ts(day: u32, sec: u64) -> String {
    let t = 8 * 3600 + sec;
    format!(
        "2026-05-{day:02}T{:02}:{:02}:{:02}.000Z",
        t / 3600,
        (t % 3600) / 60,
        t % 60
    )
}

fn line(v: &Value) -> String {
    format!("{v}\n")
}

fn assistant(req: &str, at: &str, ctx: u64, content: &[Value]) -> String {
    line(
        &json!({"type":"assistant","requestId":req,"timestamp":at,"message":{"id":format!("msg-{req}"),"model":"claude-sonnet-4-5","usage":{"input_tokens":20,"cache_read_input_tokens":ctx,"cache_creation_input_tokens":300,"output_tokens":400},"content":content}}),
    )
}

fn tool_result(id: &str, at: &str, content: &Value) -> String {
    line(
        &json!({"type":"user","timestamp":at,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":content}]}}),
    )
}

/// 120 turns: two compactions, the dumb zone crossed late, one big result.
pub fn long_session() -> String {
    let mut out = line(
        &json!({"type":"user","cwd":"/Users/demo/parser","gitBranch":"feature/speed","version":"2.0.0","sessionId":LONG_ID,"timestamp":ts(31, 0),"message":{"role":"user","content":"Tune the parser and keep the tests green.\n\n- profile first\n- then cut allocations"}}),
    );
    out += &line(
        &json!({"type":"attachment","timestamp":ts(31, 1),"attachment":{"type":"file","filename":"CLAUDE.md","content":"# Rules\nKeep functions pure.\n"}}),
    );
    let mut t = 2;
    for i in 1..=120u64 {
        let ctx = match i {
            ..=40 => 10_000 + i * 6_000,
            41..=90 => 60_000 + (i - 40) * 7_000,
            _ => 100_000 + (i - 90) * 9_000,
        };
        let id = format!("toolu_{i:03}");
        let (name, input, result) = match i % 4 {
            0 => (
                "Bash",
                json!({"command": format!("cargo test case{i}\necho done"), "description": "Run tests"}),
                json!(format!("test case{i} ... ok")),
            ),
            1 => (
                "Read",
                json!({"file_path": format!("/Users/demo/parser/src/m{i}.ts")}),
                json!([{"type":"text","text":"```ts\nexport const x = 1;\n```"}]),
            ),
            2 => (
                "Edit",
                json!({"file_path": "src/lex.ts", "old_string": "let a = 1;\nlet b = 2;", "new_string": "let a = 1;\nlet b = 3;"}),
                json!("ok"),
            ),
            _ => (
                "Grep",
                json!({"pattern": "TODO", "path": "src"}),
                json!({"matches": i, "files": ["a.ts"]}),
            ),
        };
        let result = if i == BIG_TURN {
            json!("x".repeat(BIG_BYTES - 1) + "\n")
        } else {
            result
        };
        let text = format!("Step {i}: **checking** with `{name}`.");
        out += &assistant(
            &format!("req_{i:03}"),
            &ts(31, t),
            ctx,
            &[
                json!({"type":"text","text":text}),
                json!({"type":"tool_use","id":id,"name":name,"input":input}),
            ],
        );
        t += 15;
        out += &tool_result(&id, &ts(31, t), &result);
        t += 15;
        if i == 40 || i == 90 {
            out += &line(
                &json!({"type":"system","subtype":"compact_boundary","timestamp":ts(31, t),"compactMetadata":{"trigger":"auto","preTokens":ctx}}),
            );
            out += &line(
                &json!({"type":"user","isCompactSummary":true,"timestamp":ts(31, t + 1),"message":{"role":"user","content":format!("Summary: parser work up to step {i}.")}}),
            );
            t += 2;
        }
    }
    out += &line(&json!({"type":"ai-title","timestamp":ts(31, t),"aiTitle":"Tune the parser"}));
    out
}

fn short(id: &str, day: u32, title: Option<&str>, reqs: &[&str]) -> String {
    let mut out = line(
        &json!({"type":"user","cwd":"/Users/demo/app","gitBranch":"HEAD","sessionId":id,"timestamp":ts(day, 0),"message":{"role":"user","content":"Look at the logs."}}),
    );
    for (k, req) in reqs.iter().enumerate() {
        let k = u64::try_from(k).unwrap_or(0);
        out += &assistant(
            req,
            &ts(day, 10 + k * 10),
            5_000 + k * 1_000,
            &[json!({"type":"text","text":format!("Reply {k} in {req}.")})],
        );
    }
    if let Some(t) = title {
        out += &line(&json!({"type":"ai-title","timestamp":ts(day, 99),"aiTitle":t}));
    }
    out
}

/// A line whose text holds a lone surrogate escape.
fn surrogate_session() -> String {
    let lone = format!("{}ud800", '\\');
    let mut out = short(SURROGATE_ID, 28, Some("Odd bytes"), &["s1"]);
    out += &format!(
        "{{\"type\":\"user\",\"timestamp\":\"{}\",\"message\":{{\"role\":\"user\",\"content\":\"half {lone} pair\"}}}}\n",
        ts(28, 200)
    );
    out
}

/// The full fixture HOME.
pub fn build_home() -> PathBuf {
    let home = temp_home("full");
    let projects = home.join(".claude/projects");
    copy_dir(&fixtures().join("sessions/projects"), &projects);
    write(
        &projects.join(format!("-Users-demo-parser/{LONG_ID}.jsonl")),
        &long_session(),
    );
    write(
        &projects.join(format!("-Users-demo-app/{UNTITLED_ID}.jsonl")),
        &short(UNTITLED_ID, 30, None, &["u1", "u2"]),
    );
    write(
        &projects.join(format!("-Users-demo-app/{INTERLEAVED_ID}.jsonl")),
        &short(
            INTERLEAVED_ID,
            29,
            Some("Interleaved replies"),
            &["r1", "r2", "r1", "r3"],
        ),
    );
    write(
        &projects.join(format!("-Users-demo-app/{SURROGATE_ID}.jsonl")),
        &surrogate_session(),
    );
    let codex = home.join(".codex/sessions/2026/05/20");
    fs::create_dir_all(&codex).expect("mkdir");
    fs::copy(
        fixtures().join("sessions-codex/rollout-fixture.jsonl"),
        codex.join("rollout-2026-05-20T09-00-00-019f42f4-342d-7b30-9551-a78e5a2cbe5e.jsonl"),
    )
    .expect("copy codex");
    let pi = home.join(".pi/agent/sessions/--Users-demo-proj--");
    fs::create_dir_all(&pi).expect("mkdir");
    fs::copy(
        fixtures().join("sessions-pi/pi-fixture.jsonl"),
        pi.join("2026-05-19T08-00-00-000Z_019f42f4-0000-7000-8000-000000000001.jsonl"),
    )
    .expect("copy pi");
    home
}

/// `n` tiny Claude sessions, for the virtual list.
pub fn many_home(n: usize) -> PathBuf {
    let home = temp_home("many");
    let dir = home.join(".claude/projects/-Users-demo-many");
    fs::create_dir_all(&dir).expect("mkdir");
    for i in 0..n {
        let id = format!("{i:08}-0000-4000-8000-000000000000");
        let sec = u64::try_from(i).unwrap_or(0);
        let text = line(
            &json!({"type":"user","cwd":"/Users/demo/many","sessionId":id,"timestamp":ts(10, sec),"message":{"role":"user","content":format!("Task {i}")}}),
        );
        fs::write(dir.join(format!("{id}.jsonl")), text).expect("write");
    }
    home
}
