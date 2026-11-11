//! A throwaway home with synthetic sessions and a runner for the binary,
//! as `apps/cli/test/support/synthetic-home.ts` builds them.
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

pub struct Home {
    pub dir: PathBuf,
}

impl Home {
    pub fn new(prefix: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("peektrace-{prefix}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir home");
        Self { dir }
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

/// A Codex rollout newer than every Claude session; this build cannot read it.
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
