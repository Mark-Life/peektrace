//! Throwaway HOMEs the end-to-end tests run the binary over: a preset
//! Claude project (`Fixture`) and a synthetic home built per test (`Home`,
//! as `apps/cli/test/support/synthetic-home.ts` builds them).
#![allow(dead_code)]

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{Value, json};

/// A fresh empty dir under the system temp dir, unique per call.
fn fresh_dir(prefix: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("peektrace-{prefix}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir home");
    dir
}

pub const MTIME_MS: u64 = 1_794_415_341_851;
pub const MTIME_ISO: &str = "2026-11-11T16:42:21.851Z";

pub const S1: &str = r#"{"type":"user","timestamp":"2026-01-01T10:00:00.000Z","cwd":"","gitBranch":"main","message":{"role":"user","content":"hi"}}
{"type":"user","timestamp":"2026-01-01T10:00:01.000Z","cwd":"/tmp/proj","isMeta":true,"message":{"content":"meta"}}
{"type":"assistant","timestamp":"2026-01-01T10:00:02.000Z","requestId":"r1","message":{"model":"<synthetic>","usage":{"input_tokens":5}}}
{"type":"assistant","timestamp":"2026-01-01T10:00:03.000Z","requestId":"r2","message":{"model":"claude-opus-5-5","usage":{"input_tokens":1000,"cache_read_input_tokens":2000,"cache_creation_input_tokens":3000,"cache_creation":{"ephemeral_1h_input_tokens":1000},"output_tokens":10}}}
{"type":"assistant","timestamp":"2026-01-01T10:00:04.000Z","requestId":"r2","message":{"model":"claude-opus-5-5","usage":{"input_tokens":1000,"output_tokens":50}}}
{"type":"assistant","timestamp":"2026-01-01T10:00:05.000Z","message":{"model":"mystery-model","usage":{"input_tokens":7}}}
{"type":"user","timestamp":"2026-01-01T10:00:06.000Z","message":{"content":[{"type":"tool_result","content":"x"}]}}
not json
{"type":"ai-title","aiTitle":"First title"}
{"type":"ai-title","aiTitle":"Final	title"}
{"type":"user","timestamp":"2026-01-01T09:00:00.000Z","gitBranch":"feat","message":{"content":"later line older ts"}}
{"type":"user","timestamp":"#;

pub const SUBAGENT_Y: &str = r#"{"type":"user","timestamp":"2026-01-01T10:00:02.500Z","isSidechain":true,"message":{"content":"look"}}
{"type":"assistant","timestamp":"2026-01-01T10:00:02.600Z","isSidechain":true,"requestId":"q1","message":{"model":"claude-opus-5-5","usage":{"input_tokens":400,"output_tokens":20},"content":[{"type":"text","text":"found it"}]}}
"#;

/// A throwaway HOME with one Claude project.
pub struct Fixture {
    pub home: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let home = fresh_dir("cli");
        let proj = home.join(".claude/projects/-tmp-proj");
        fs::create_dir_all(proj.join("s1/subagents")).expect("mkdir");
        let write = |name: &str, body: &str| {
            let path = proj.join(name);
            fs::write(&path, body).expect("write");
            set_mtime(&path);
        };
        write("s1.jsonl", S1);
        write("s1/subagents/agent-x.jsonl", "{\"type\":\"user\"}\n");
        write("s1/subagents/agent-y.jsonl", SUBAGENT_Y);
        write(
            "s1/subagents/agent-y.meta.json",
            r#"{"agentType":"Explore","description":"Find the file"}"#,
        );
        write(
            "bad.jsonl",
            "{\"type\":\"user\",\"message\":{\"content\":\"x\"}}\nbad\n{\"type\":\"user\"}\n",
        );
        write("ws.jsonl", "\n  \n");
        write("empty.jsonl", "");
        write("notes.txt", "x");
        Self { home }
    }

    pub fn run(&self, args: &[impl AsRef<std::ffi::OsStr>]) -> (i32, String, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_peektrace"))
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env("PEEKTRACE_DIR", self.home.join(".peektrace"))
            .current_dir(&self.home)
            .output()
            .expect("run peektrace");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

pub fn set_mtime(path: &Path) {
    File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(UNIX_EPOCH + Duration::from_millis(MTIME_MS)))
        .expect("set mtime");
}

impl Fixture {
    pub fn settings(&self, body: &[u8]) -> PathBuf {
        let dir = self.home.join(".peektrace");
        fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("settings.json");
        fs::write(&path, body).expect("write settings");
        path
    }
}

pub struct Home {
    pub dir: PathBuf,
}

impl Home {
    pub fn new(prefix: &str) -> Self {
        Self {
            dir: fresh_dir(prefix),
        }
    }

    pub fn str(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }

    /// `lines` as JSONL at `path` (relative to the home).
    pub fn write(&self, rel: &str, lines: &[Value]) -> PathBuf {
        let path = self.dir.join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let body: Vec<String> = lines.iter().map(Value::to_string).collect();
        fs::write(&path, format!("{}\n", body.join("\n"))).expect("write");
        path
    }

    /// Run the binary with a clean env: nothing from the developer's machine
    /// leaks in.
    pub fn run(&self, args: &[&str]) -> Run {
        let h = &self.dir;
        let out = Command::new(env!("CARGO_BIN_EXE_peektrace"))
            .args(args)
            .env_clear()
            .env("HOME", h)
            .env("CLAUDE_CONFIG_DIR", h.join(".claude"))
            .env("CODEX_HOME", h.join(".codex"))
            .env("XDG_DATA_HOME", h.join(".local/share"))
            .env("PEEKTRACE_DIR", h.join(".peektrace"))
            .env("PEEKTRACE_NO_TELEMETRY", "1")
            .current_dir(h)
            .output()
            .expect("run peektrace");
        Run {
            code: out.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            err: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

pub struct Run {
    pub code: i32,
    pub out: String,
    pub err: String,
}

pub fn claude_path(slug: &str, id: &str) -> String {
    format!(".claude/projects/{slug}/{id}.jsonl")
}

/// A user line then one assistant turn of `context_tokens`.
pub fn claude_lines(at: &str, context_tokens: u32, cwd: &str) -> Vec<Value> {
    vec![
        json!({"type":"user","cwd":cwd,"timestamp":at,"message":{"content":"hello"}}),
        json!({"type":"assistant","cwd":cwd,"timestamp":at,"requestId":"req-1",
            "message":{"model":"claude-opus-4-1","content":[{"type":"text","text":"hi"}],
                "usage":{"input_tokens":context_tokens,"cache_read_input_tokens":0,
                    "cache_creation_input_tokens":0,"output_tokens":10}}}),
    ]
}

/// A Codex rollout newer than every Claude session.
pub fn codex_lines(id: &str, cwd: &str) -> Vec<Value> {
    vec![
        json!({"timestamp":"2026-06-01T10:00:00.000Z","type":"session_meta",
            "payload":{"id":id,"timestamp":"2026-06-01T10:00:00.000Z","cwd":cwd}}),
        json!({"timestamp":"2026-06-01T10:00:00.000Z","type":"response_item",
            "payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello codex"}]}}),
    ]
}

pub fn codex_path(id: &str) -> String {
    format!(".codex/sessions/2026/06/01/rollout-2026-06-01T10-00-00-{id}.jsonl")
}
