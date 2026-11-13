//! A synthetic HOME for snapshot and key tests: the core fixtures under
//! every agent root, a generated 150-turn Claude session, memory vaults
//! and a settings file. Never the real home.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

/// `2026-11-11T17:00:00Z`.
pub const NOW_MS: f64 = 1_794_416_400_000.0;
pub const LONG_ID: &str = "22222222-2222-4222-8222-222222222222";

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

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

/// A fresh HOME under the target dir.
pub fn temp_home() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "tui-home-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir home");
    dir
}

/// An empty HOME.
pub fn empty_home() -> PathBuf {
    temp_home()
}

/// The full fixture HOME.
pub fn build_home() -> PathBuf {
    let home = temp_home();
    let projects = home.join(".claude/projects");
    copy_dir(&fixtures().join("sessions/projects"), &projects);
    let other = projects.join("-Users-demo-other");
    fs::create_dir_all(&other).expect("mkdir");
    for name in [
        "prompt-origin",
        "retries",
        "server-tools",
        "thinking-tokens",
    ] {
        let src = fixtures().join(format!("sessions-claude/{name}.jsonl"));
        fs::copy(&src, other.join(format!("{name}.jsonl"))).expect("copy");
    }
    write(
        &projects.join(format!("-Users-demo-long/{LONG_ID}.jsonl")),
        &long_session(),
    );
    let codex = home.join(".codex/sessions/2026/11/10");
    fs::create_dir_all(&codex).expect("mkdir");
    fs::copy(
        fixtures().join("sessions-codex/rollout-fixture.jsonl"),
        codex.join("rollout-2026-11-10T09-00-00-019f42f4-342d-7b30-9551-a78e5a2cbe5e.jsonl"),
    )
    .expect("copy codex");
    let pi = home.join(".pi/agent/sessions/--Users-demo-proj--");
    fs::create_dir_all(&pi).expect("mkdir");
    fs::copy(
        fixtures().join("sessions-pi/pi-context-edit.jsonl"),
        pi.join("2026-11-09T08-00-00-000Z_019f42f4-0000-7000-8000-000000000001.jsonl"),
    )
    .expect("copy pi");
    memory(&projects);
    pin_mtimes(&home);
    write(
        &home.join(".peektrace/settings.json"),
        &serde_json::to_string_pretty(&json!({
            "roots": {
                "claude": [{"path": "~/work/.claude", "label": "work"}],
                "codex": [{"path": "~/work/.codex"}]
            }
        }))
        .expect("json"),
    );
    home
}

/// `2026-11-11T16:42:21.851Z` on every file, so dates in snapshots hold.
fn pin_mtimes(dir: &Path) {
    let at = std::time::UNIX_EPOCH + std::time::Duration::from_millis(1_794_415_341_851);
    for e in fs::read_dir(dir).expect("read dir") {
        let path = e.expect("entry").path();
        if path.is_dir() {
            pin_mtimes(&path);
        } else {
            fs::File::options()
                .write(true)
                .open(&path)
                .and_then(|f| f.set_modified(at))
                .expect("set mtime");
        }
    }
}

fn memory(projects: &Path) {
    let vault = projects.join("-Users-demo-proj/memory");
    write(
        &vault.join("MEMORY.md"),
        "- [Style](style.md) — keep functions pure\n- [Build](build.md) — how to build\n- [Gone](gone.md) — removed file\n",
    );
    write(
        &vault.join("style.md"),
        "---\nname: Style\ndescription: How code in this repo is written: pure functions, small modules, and tests beside each module.\ntype: feedback\n---\nPrefer pure functions.\n\nSee [build](build.md) and [tests](tests.md).\n",
    );
    write(
        &vault.join("build.md"),
        "---\nname: Build\ntype: project\n---\nRun `cargo build --release`.\n",
    );
    write(
        &vault.join("notes.md"),
        "Untyped scratch notes.\nLine two.\n",
    );
    let big = projects.join("-Users-demo-other/memory");
    let index = (0..230).fold(String::new(), |mut acc, i| {
        let _ = writeln!(acc, "- [Entry {i}](e{i}.md) — line {i}");
        acc
    });
    write(&big.join("MEMORY.md"), &index);
    write(
        &big.join("e1.md"),
        "---\nname: User prefs\ntype: user\n---\nShort answers.\n",
    );
    write(
        &big.join("e2.md"),
        "---\nname: Docs link\ntype: reference\n---\nhttps://example.test/docs\n",
    );
}

/// `2026-11-11T09:00:00Z` plus `sec`; the session stays within the day.
fn ts(sec: u64) -> String {
    let t = 9 * 3600 + sec;
    format!(
        "2026-11-11T{:02}:{:02}:{:02}.000Z",
        t / 3600,
        (t % 3600) / 60,
        t % 60
    )
}

fn line(v: &Value) -> String {
    format!("{v}\n")
}

/// 150 turns: context grows past the dumb zone, compacts at turn 90, and
/// the bodies cover Bash (with a heredoc), JSON and text-block results,
/// markdown, empty thinking, an attachment and escape codes.
pub fn long_session() -> String {
    let mut out = String::new();
    let cwd = "/Users/demo/long";
    out += &line(
        &json!({"type":"user","cwd":cwd,"gitBranch":"main","version":"2.0.0","sessionId":LONG_ID,"timestamp":ts(0),"message":{"role":"user","content":"Fix the flaky build and tidy the scripts."}}),
    );
    out += &line(
        &json!({"type":"attachment","timestamp":ts(1),"attachment":{"type":"deferred_tools_delta","content":"The following deferred tools are now available: Monitor, WebFetch, WebSearch."}}),
    );
    let mut t = 2;
    for i in 1..=150u64 {
        let ctx = if i <= 90 {
            4_000 + i * 1_400
        } else {
            30_000 + (i - 90) * 900
        };
        let tool = format!("toolu_{i:03}");
        let (name, input, result): (&str, Value, Value) = match i % 6 {
            0 => (
                "Bash",
                json!({"command": "cat <<'EOF' > notes.txt\nline one\nline two\nEOF\nwc -l notes.txt"}),
                json!("2 notes.txt"),
            ),
            1 => (
                "Bash",
                json!({"command": format!("npm test -- --grep case{i} 2>&1 | tail -5")}),
                json!(format!(
                    "\u{1b}[31mFAIL\u{1b}[0m case{i}\n\u{1b}]8;;http://x\u{7}link\u{1b}]8;;\u{1b}\\ done"
                )),
            ),
            2 => (
                "Read",
                json!({"file_path": format!("/Users/demo/long/src/mod{i}.ts")}),
                json!([{"type":"text","text":"## Module\n\n```ts\nexport const x = 1;\n```"}]),
            ),
            3 => (
                "Grep",
                json!({"pattern": "TODO", "path": "src"}),
                json!({"matches": i, "files": ["a.ts", "b.ts"]}),
            ),
            4 => (
                "mcp__executor__execute",
                json!({"code": "const r = await tools.search({ query: \"x\" });\nreturn r.length;"}),
                json!("3"),
            ),
            _ => (
                "Edit",
                json!({"file_path": "src/a.ts", "old_string": "a", "new_string": "b"}),
                json!("ok"),
            ),
        };
        let mut content = vec![];
        if i % 10 == 0 {
            content.push(json!({"type":"thinking","thinking":""}));
        }
        content.push(json!({"type":"text","text":format!("Step {i}: checking with `{name}`.")}));
        content.push(json!({"type":"tool_use","id":tool,"name":name,"input":input}));
        out += &line(
            &json!({"type":"assistant","requestId":format!("req_{i:03}"),"timestamp":ts(t),"message":{"id":format!("msg_{i:03}"),"model":"claude-sonnet-4-5","usage":{"input_tokens":20,"cache_read_input_tokens":ctx,"cache_creation_input_tokens":300,"output_tokens":120},"content":content}}),
        );
        t += 20;
        let is_error = i % 12 == 1;
        out += &line(
            &json!({"type":"user","timestamp":ts(t),"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":tool,"content":result,"is_error":is_error}]}}),
        );
        t += 20;
        if i == 90 {
            out += &line(
                &json!({"type":"system","subtype":"compact_boundary","timestamp":ts(t),"compactMetadata":{"trigger":"auto","preTokens":ctx}}),
            );
            out += &line(
                &json!({"type":"user","isCompactSummary":true,"timestamp":ts(t + 1),"message":{"role":"user","content":"Summary: the build was fixed up to step 90."}}),
            );
            t += 2;
        }
    }
    out += &line(&json!({"type":"ai-title","timestamp":ts(t),"aiTitle":"Fix the flaky build"}));
    out
}
