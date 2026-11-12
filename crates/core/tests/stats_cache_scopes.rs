//! The shard cache across scoped and full stats calls.
//!
//! A temp home holds one Claude and one Codex transcript, both synthetic. A
//! pass narrowed to one agent must leave the other agent's shards warm, and a
//! full pass must drop the shard of a transcript that no longer exists. A
//! window that starts after every fact of a shard skips reading it, but keeps
//! its index entry and its shard file.

mod common;

use common::TempDir;
use peektrace_core::js::time::Utc;
use peektrace_core::roots::{RootsEnv, Settings};
use peektrace_core::stats::cache::StatsCache;
use peektrace_core::stats::service::{
    RefreshSummary, ReportRequest, StatsEnv, StatsReport, refresh, report,
};
use peektrace_core::{Agent, AgentRoots};
use serde_json::{Value, json};

const SLUG: &str = "-work-demo";
const CLAUDE_SID: &str = "33333333-3333-4333-8333-333333333333";
const CODEX_SID: &str = "44444444-4444-4444-8444-444444444444";
/// 2026-07-01T12:00:00Z.
const FIXED_MS: f64 = 1_782_907_200_000.0;

fn jsonl(rows: &[Value]) -> String {
    rows.iter().map(|r| r.to_string() + "\n").collect()
}

fn claude_text() -> String {
    jsonl(&[
        json!({"type": "user", "cwd": "/work/demo", "timestamp": "2026-06-01T10:00:00.000Z",
            "message": {"content": "list the files"}}),
        json!({"type": "assistant", "requestId": "req-1", "timestamp": "2026-06-01T10:00:05.000Z",
            "message": {"model": "claude-opus-4", "usage": {"input_tokens": 10, "output_tokens": 5},
                "content": [{"type": "text", "text": "done"}]}}),
    ])
}

fn codex_text() -> String {
    jsonl(&[
        json!({"timestamp": "2026-06-02T10:00:00.000Z", "type": "session_meta", "payload": {
            "id": CODEX_SID, "timestamp": "2026-06-02T10:00:00.000Z", "cwd": "/work/demo",
            "cli_version": "0.1.0", "model_provider": "openai"}}),
        json!({"timestamp": "2026-06-02T10:00:01.000Z", "type": "turn_context",
            "payload": {"turn_id": "turn-1", "cwd": "/work/demo", "model": "gpt-5"}}),
        json!({"timestamp": "2026-06-02T10:00:02.000Z", "type": "response_item", "payload": {
            "type": "message", "role": "user", "content": [{"type": "input_text", "text": "list the files"}]}}),
        json!({"timestamp": "2026-06-02T10:00:03.000Z", "type": "response_item", "payload": {
            "type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "done"}]}}),
    ])
}

struct Corpus {
    dir: TempDir,
    roots: AgentRoots,
    claude: String,
    codex: String,
}

impl Corpus {
    fn new(tag: &str) -> Self {
        let dir = TempDir::new(tag);
        dir.mkdir("oc");
        let claude = dir.write(
            &format!("home/.claude/projects/{SLUG}/{CLAUDE_SID}.jsonl"),
            &claude_text(),
        );
        let codex = dir.write(
            &format!(
                ".codex-home/sessions/2026/06/02/rollout-2026-06-02T10-00-00-{CODEX_SID}.jsonl"
            ),
            &codex_text(),
        );
        let env = RootsEnv {
            home: dir.join("home"),
            codex_home: Some(dir.join(".codex-home")),
            opencode_data: Some(dir.join("oc")),
            ..RootsEnv::default()
        };
        let roots = AgentRoots::resolve(&env, &Settings::default());
        Self {
            dir,
            roots,
            claude,
            codex,
        }
    }

    fn cache_dir(&self) -> String {
        self.dir.join("pt/stats-rs")
    }

    fn env(&self) -> StatsEnv<'_> {
        StatsEnv {
            roots: &self.roots,
            cache: StatsCache::new(self.cache_dir()),
            local: &Utc,
            now_ms: FIXED_MS,
            progress: None,
        }
    }

    fn refresh(&self, agents: Option<&[Agent]>, force: bool) -> RefreshSummary {
        refresh(&self.env(), agents, force).expect("refresh")
    }

    fn report(&self, agents: Option<Vec<Agent>>, days: Option<f64>, force: bool) -> StatsReport {
        let req = ReportRequest {
            agents,
            days,
            force,
            limit: None,
        };
        report(&self.env(), &req).expect("report")
    }

    fn index_paths(&self) -> Vec<String> {
        let mut v: Vec<String> = StatsCache::new(self.cache_dir())
            .load()
            .keys()
            .cloned()
            .collect();
        v.sort();
        v
    }

    fn both(&self) -> Vec<String> {
        let mut v = vec![self.claude.clone(), self.codex.clone()];
        v.sort();
        v
    }

    fn shard_names(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(format!("{}/shards", self.cache_dir()))
            .expect("shards")
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .collect();
        v.sort();
        v
    }

    /// Overwrite every shard, so a pass that reads one has to re-extract it.
    fn corrupt_shards(&self) {
        for name in self.shard_names() {
            std::fs::write(format!("{}/shards/{name}", self.cache_dir()), "not json")
                .expect("corrupt");
        }
    }

    fn shard_bodies(&self) -> Vec<String> {
        self.shard_names()
            .iter()
            .map(|n| {
                std::fs::read_to_string(format!("{}/shards/{n}", self.cache_dir())).expect("shard")
            })
            .collect()
    }
}

const CODEX: &[Agent] = &[Agent::Codex];

#[test]
fn a_scoped_refresh_keeps_the_other_agent_cached() {
    let c = Corpus::new("cache-scoped-refresh");
    let first = c.refresh(None, false);
    assert_eq!((first.transcripts, first.extracted), (2, 2));
    let scoped = c.refresh(Some(CODEX), false);
    assert_eq!((scoped.transcripts, scoped.cached), (1, 1));
    let full = c.refresh(None, false);
    assert_eq!((full.extracted, full.cached), (0, 2));
}

#[test]
fn a_scoped_report_keeps_the_other_agent_cached() {
    let c = Corpus::new("cache-scoped-report");
    c.refresh(None, false);
    c.report(Some(CODEX.to_vec()), None, false);
    assert_eq!(c.index_paths(), c.both());
    let full = c.refresh(None, false);
    assert_eq!((full.extracted, full.cached), (0, 2));
}

#[test]
fn a_scoped_force_re_extracts_only_its_own_agent() {
    let c = Corpus::new("cache-scoped-force");
    c.refresh(None, false);
    let forced = c.refresh(Some(CODEX), true);
    assert_eq!(forced.extracted, 1);
    let full = c.refresh(None, false);
    assert_eq!((full.extracted, full.cached), (0, 2));
}

#[test]
fn a_full_pass_drops_the_shard_of_a_deleted_transcript() {
    let c = Corpus::new("cache-deleted");
    c.refresh(None, false);
    assert_eq!(c.shard_names().len(), 2);
    std::fs::remove_file(&c.claude).expect("rm");
    let full = c.refresh(None, false);
    assert_eq!((full.transcripts, full.cached), (1, 1));
    assert_eq!(c.index_paths(), std::slice::from_ref(&c.codex));
    assert_eq!(c.shard_names().len(), 1);
}

#[test]
fn a_narrow_full_report_keeps_every_index_entry_and_shard() {
    let c = Corpus::new("cache-narrow-full");
    c.refresh(None, false);
    let narrow = c.report(None, Some(1.0), false);
    assert_eq!(narrow.corpus.transcripts, 0.0);
    assert_eq!(c.index_paths(), c.both());
    assert_eq!(c.shard_names().len(), 2);
    let wide = c.refresh(None, false);
    assert_eq!((wide.extracted, wide.cached), (0, 2));
    assert!(wide.facts > 0);
}

#[test]
fn a_narrow_scoped_report_keeps_every_index_entry() {
    let c = Corpus::new("cache-narrow-scoped");
    c.refresh(None, false);
    c.report(Some(vec![Agent::Claude]), Some(1.0), false);
    assert_eq!(c.index_paths(), c.both());
    let wide = c.refresh(None, false);
    assert_eq!((wide.extracted, wide.cached), (0, 2));
}

#[test]
fn a_shard_with_nothing_in_the_window_is_not_read() {
    let c = Corpus::new("cache-window-skip");
    c.refresh(None, false);
    c.corrupt_shards();
    // A read re-extracts and rewrites a corrupt shard, so unchanged bodies
    // mean no read.
    let narrow = c.report(None, Some(1.0), false);
    assert_eq!(narrow.corpus.transcripts, 0.0);
    assert_eq!(c.shard_bodies(), ["not json", "not json"]);
    assert_eq!(c.index_paths(), c.both());
}

#[test]
fn a_cold_narrow_report_still_writes_shards_a_wider_run_reads() {
    let c = Corpus::new("cache-cold-narrow");
    c.report(None, Some(1.0), false);
    assert_eq!(c.index_paths(), c.both());
    let wide = c.refresh(None, false);
    assert_eq!((wide.extracted, wide.cached), (0, 2));
}

#[test]
fn cold_and_warm_reports_match_inside_and_outside_the_window() {
    let c = Corpus::new("cache-cold-warm");
    for days in [1.0, 60.0] {
        let cold = c.report(None, Some(days), true).to_json(false);
        let warm = c.report(None, Some(days), false).to_json(false);
        assert_eq!(warm, cold, "days {days}");
    }
}

#[test]
fn a_corrupt_shard_is_re_extracted_and_rewritten_not_a_failed_pass() {
    let c = Corpus::new("cache-corrupt");
    c.refresh(None, false);
    c.corrupt_shards();
    let wide = c.refresh(None, false);
    assert_eq!(wide.extracted, 2);
    assert!(!c.shard_bodies().iter().any(|b| b == "not json"));
    let again = c.refresh(None, false);
    assert_eq!(again.cached, 2);
}

#[test]
fn a_scoped_pass_keeps_entries_another_pass_wrote_while_it_ran() {
    let c = Corpus::new("cache-side-by-side");
    // The Claude pass commits while the Codex pass is between its fold and
    // its own commit.
    let claude_pass = |_: usize, _: usize| {
        c.refresh(Some(&[Agent::Claude]), false);
    };
    let env = StatsEnv {
        progress: Some(&claude_pass),
        ..c.env()
    };
    let codex = refresh(&env, Some(CODEX), false).expect("refresh");
    assert_eq!(codex.extracted, 1);
    assert_eq!(c.index_paths(), c.both());
    let full = c.refresh(None, false);
    assert_eq!((full.extracted, full.cached), (0, 2));
}
