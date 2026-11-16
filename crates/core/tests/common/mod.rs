//! Helpers shared by the ported TS tests: session parsing, throwaway temp
//! dirs, `OpenCode` data dirs and SQLite stores built with the DDL of
//! `packages/core/test/fixtures/opencode-db.ts`, and (in `stats`) synthetic
//! stats facts and transcripts.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use peektrace_core::claude::{ParseArgs, parse_claude_session};
use peektrace_core::js::time::Utc;
use peektrace_core::roots;
use peektrace_core::roots::{RootsEnv, Source, claude_sources};
use peektrace_core::sessions::analyze::{AnalyzeOptions, analyze};
use peektrace_core::sessions::model::{AnalyzedSession, EventKind, ParsedSession, TimelineEvent};
use peektrace_core::sessions::service::SessionEnv;
use peektrace_core::{
    Agent, AgentRoots, FileStamp, ListFilter, ProjectSlug, SessionFileRef, SessionHeader,
    SessionId, list_sessions,
};
use rusqlite::{Connection, params};
use serde_json::Value;

pub mod causes;
pub mod rules;
mod stats;

pub use stats::*;

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

/// Claude roots with `projects` as the default projects root.
pub fn sources(home: &str, projects: &str) -> Vec<Source> {
    let env = RootsEnv {
        home: home.to_owned(),
        claude_projects: Some(projects.to_owned()),
        ..RootsEnv::default()
    };
    claude_sources(&env, &[])
}

/// Roots with only the Claude sources of [`sources`].
pub fn claude_roots(home: &str, projects: &str) -> AgentRoots {
    AgentRoots {
        claude: sources(home, projects),
        ..AgentRoots::default()
    }
}

const SESSION_DDL: &str = "CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT, directory TEXT, title TEXT, version TEXT, model TEXT, agent TEXT, cost REAL, tokens_input INTEGER, tokens_output INTEGER, tokens_reasoning INTEGER, tokens_cache_read INTEGER, tokens_cache_write INTEGER, time_created INTEGER, time_updated INTEGER)";
const MESSAGE_DDL: &str = "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT)";
const PART_DDL: &str = "CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT)";

pub fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

pub fn create_db(path: &Path) -> Connection {
    let db = Connection::open(path).expect("open");
    for ddl in [SESSION_DDL, MESSAGE_DDL, PART_DDL] {
        db.execute(ddl, []).expect("ddl");
    }
    db
}

/// `writeGoodDb`: one text-only session.
pub fn write_good_db(path: &Path, id: &str, directory: &str) {
    let db = create_db(path);
    db.execute(
        "INSERT INTO session (id, directory, title, time_created, time_updated) VALUES (?, ?, ?, ?, ?)",
        params![id, directory, "Good session", 1000, 2000],
    )
    .expect("session");
    db.execute(
        "INSERT INTO message (id, session_id, time_created, data) VALUES (?, ?, ?, ?)",
        params![
            format!("{id}-m"),
            id,
            1500,
            r#"{"role":"user","time":{"created":1500}}"#
        ],
    )
    .expect("message");
    db.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, data) VALUES (?, ?, ?, ?, ?)",
        params![
            format!("{id}-p"),
            format!("{id}-m"),
            id,
            1501,
            format!(r#"{{"type":"text","text":"hi from {id}"}}"#)
        ],
    )
    .expect("part");
}

pub fn write_json(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, body).expect("write");
}

/// Roots with only `data` as the `OpenCode` data dir.
pub fn opencode_roots(data: &str) -> AgentRoots {
    let env = RootsEnv {
        home: "/nonexistent-home".to_owned(),
        opencode_data: Some(data.to_owned()),
        ..RootsEnv::default()
    };
    AgentRoots {
        opencode: roots::sources(Agent::Opencode, &env, &[]),
        ..AgentRoots::default()
    }
}

/// `OpenCode` list headers under `data`.
pub fn headers(data: &str) -> Vec<SessionHeader> {
    list_sessions(
        &opencode_roots(data),
        &ListFilter {
            agent: Some(Agent::Opencode),
            project: None,
        },
    )
    .headers
}

pub fn ids(refs: &[SessionFileRef]) -> Vec<&str> {
    refs.iter().map(|r| r.id.as_str()).collect()
}

pub fn session_env(roots: &AgentRoots) -> SessionEnv<'_> {
    SessionEnv { roots, local: &Utc }
}

/// A canonical [`temp_dir`] removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        Self(std::fs::canonicalize(temp_dir(tag)).expect("canonical temp dir"))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn str(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }

    /// The full path of `rel`.
    pub fn join(&self, rel: &str) -> String {
        self.0.join(rel).to_string_lossy().into_owned()
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

/// The file ref a header builder takes (`HEADER_ARGS` in the TS tests).
pub fn file_ref(id: &str, slug: &str, path: &str, size_bytes: u64) -> SessionFileRef {
    SessionFileRef {
        id: SessionId::new(id),
        slug: ProjectSlug::new(slug),
        path: path.to_owned(),
        stamp: FileStamp {
            size_bytes,
            mtime_ms: 0,
        },
        source: None,
    }
}

/// A fresh empty dir under the system temp dir, unique per call; the
/// caller removes it.
pub fn temp_dir(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("peektrace-test-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir
}

/// `JSON.stringify(v, null, 2)` for a flat object of string values.
pub fn pretty(pairs: &[(&str, &str)]) -> String {
    let body: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("  \"{k}\": \"{v}\""))
        .collect();
    format!("{{\n{}\n}}", body.join(",\n"))
}

/// Set a file's mtime to `ms` since the epoch.
pub fn set_mtime(path: &str, ms: u64) {
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms);
    std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(t))
        .expect("set mtime");
}
