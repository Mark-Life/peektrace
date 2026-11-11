//! Helpers shared by the ported TS session tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use peektrace_core::claude::{ParseArgs, parse_claude_session};
use peektrace_core::js::time::Utc;
use peektrace_core::sessions::analyze::{AnalyzeOptions, analyze};
use peektrace_core::sessions::model::{AnalyzedSession, EventKind, ParsedSession, TimelineEvent};
use serde_json::Value;

/// A path under `packages/core/test/fixtures`.
pub fn fixture(sub: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/core/test/fixtures")
        .join(sub)
        .to_string_lossy()
        .into_owned()
}

pub fn read_fixture(sub: &str) -> String {
    std::fs::read_to_string(fixture(sub)).expect("read fixture")
}

/// Rows as `rows.map(JSON.stringify).join("\n")`.
pub fn jsonl(rows: &[Value]) -> String {
    rows.iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_text(text: &str) -> ParsedSession {
    parse_claude_session(&ParseArgs {
        text,
        path: "/demo/s.jsonl",
        session_id: "s",
        ..ParseArgs::default()
    })
}

pub fn parse_rows(rows: &[Value]) -> ParsedSession {
    parse_text(&jsonl(rows))
}

pub fn parse_sidechain(text: &str) -> ParsedSession {
    parse_claude_session(&ParseArgs {
        text,
        path: "/demo/s.jsonl",
        session_id: "s",
        include_sidechain_turns: true,
        ..ParseArgs::default()
    })
}

pub fn parse_fixture(sub: &str) -> ParsedSession {
    let path = fixture(sub);
    let text = read_fixture(sub);
    parse_claude_session(&ParseArgs {
        text: &text,
        path: &path,
        session_id: "s1",
        ..ParseArgs::default()
    })
}

pub fn analyze_default(p: ParsedSession) -> AnalyzedSession {
    analyze(
        p,
        AnalyzeOptions {
            dumb_zone_fraction: None,
            on_disk_context_files: Vec::new(),
            window: None,
            local: &Utc,
        },
    )
}

pub fn of_kind(p: &ParsedSession, kind: EventKind) -> Vec<&TimelineEvent> {
    p.events.iter().filter(|e| e.kind == kind).collect()
}

pub fn first_of(p: &ParsedSession, kind: EventKind) -> &TimelineEvent {
    p.events
        .iter()
        .find(|e| e.kind == kind)
        .unwrap_or_else(|| panic!("no {} event", kind.as_str()))
}

/// A fresh directory under the system temp dir, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("peektrace-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir temp");
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn str(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }

    /// Write `body` at `rel`, creating parent dirs; returns the full path.
    pub fn write(&self, rel: &str, body: &str) -> String {
        let p = self.0.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&p, body).expect("write");
        p.to_string_lossy().into_owned()
    }

    pub fn mkdir(&self, rel: &str) -> String {
        let p = self.0.join(rel);
        std::fs::create_dir_all(&p).expect("mkdir");
        p.to_string_lossy().into_owned()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
