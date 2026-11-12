//! Per-file fact cache: `index.json` plus one shard of facts per transcript,
//! keyed on path, mtime, size and extractor version.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use indexmap::IndexMap;

use super::rule_dates::LineLedger;
use super::schema::{
    BashPart, CallFact, CauseId, EXTRACTOR_VERSION, Fact, FactBase, Job, Kind, SessionFact,
    UsageFact, WaitShape, fact_json,
};
use crate::js::json_out::Writer;
use crate::js::time::{LocalZone, parse_date};
use crate::js::value::{JsObject, JsValue};
use crate::model::Agent;
use crate::roots::RootsEnv;

const INDEX_VERSION: f64 = 1.0;
const LEDGER_VERSION: f64 = 1.0;
const SHARD_HASH_LEN: usize = 16;

/// Size and mtime of a transcript, as `fs.stat` reports them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FileStat {
    pub size: f64,
    pub mtime_ms: f64,
}

/// One cached transcript summary.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheEntry {
    pub path: String,
    pub mtime_ms: f64,
    pub size: f64,
    pub shard: String,
    pub rows: f64,
    pub last_fact_ms: Option<f64>,
    pub extractor_version: f64,
}

/// `sha1(s)` as hex, over the UTF-8 of `s` (lone surrogates as U+FFFD).
pub fn sha1_hex(s: &str) -> String {
    let text = crate::js::text::to_display(s);
    sha1_smol::Sha1::from(text.as_bytes()).digest().to_string()
}

pub fn is_cacheable(stat: FileStat) -> bool {
    stat.size > 0.0 || stat.mtime_ms > 0.0
}

/// A fact's own time in epoch ms.
#[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
pub fn fact_ms(fact: &Fact, local: &dyn LocalZone) -> Option<f64> {
    let ts = match fact {
        Fact::Session(s) => s.started_at.as_deref(),
        Fact::Call(c) => c.ts.as_deref(),
        Fact::Usage(u) => u.ts.as_deref(),
    }?;
    parse_date(ts, local).map(|ms| ms as f64)
}

/// Newest fact time, or `None` as soon as one fact is undated.
pub fn last_fact_ms_of(facts: &[Fact], local: &dyn LocalZone) -> Option<f64> {
    facts
        .iter()
        .try_fold(0.0f64, |last, f| fact_ms(f, local).map(|ms| last.max(ms)))
}

/// Every fact of the entry is older than the window start.
pub fn is_past_window(entry: &CacheEntry, from_ms: f64) -> bool {
    entry.last_fact_ms.is_some_and(|l| l < from_ms)
}

pub fn is_fresh(entry: &CacheEntry, path: &str, stat: FileStat) -> bool {
    is_cacheable(stat)
        && entry.path == path
        && entry.mtime_ms == stat.mtime_ms
        && entry.size == stat.size
        && entry.extractor_version == EXTRACTOR_VERSION
}

/// The previous index minus every scanned path, plus this pass's entries.
pub fn merge_index(
    prev: &IndexMap<String, CacheEntry>,
    entries: &[CacheEntry],
    scanned: &[String],
) -> Vec<CacheEntry> {
    let mut next = prev.clone();
    for p in scanned {
        next.shift_remove(p);
    }
    for e in entries {
        next.insert(e.path.clone(), e.clone());
    }
    next.into_values().collect()
}

/// The on-disk cache under one root dir.
#[derive(Debug, Clone)]
pub struct StatsCache {
    pub root: String,
    index_path: String,
    shards_dir: String,
    lines_path: String,
}

fn num(o: &JsObject, k: &str) -> Option<f64> {
    o.get(k).and_then(JsValue::as_num)
}

fn opt_str(o: &JsObject, k: &str) -> Option<String> {
    o.get(k).and_then(JsValue::as_str).map(str::to_owned)
}

fn str_of(o: &JsObject, k: &str) -> String {
    opt_str(o, k).unwrap_or_default()
}

fn strs(o: &JsObject, k: &str) -> Vec<String> {
    o.get(k)
        .and_then(JsValue::as_arr)
        .map(|a| {
            a.iter()
                .filter_map(JsValue::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn base_of(o: &JsObject) -> Option<FactBase> {
    Some(FactBase {
        sid: str_of(o, "sid"),
        agent: Agent::parse(o.get("agent")?.as_str()?)?,
        project: str_of(o, "project"),
        kind: Kind::parse(o.get("kind")?.as_str()?)?,
        parent: opt_str(o, "parent"),
        run: opt_str(o, "run"),
    })
}

fn call_of(o: &JsObject, bash: bool) -> Option<CallFact> {
    let flag = |k: &str| o.get(k).is_some_and(JsValue::is_true);
    let bash = if bash {
        Some(Box::new(BashPart {
            cmd: str_of(o, "cmd"),
            ran: strs(o, "ran"),
            bin: str_of(o, "bin"),
            family: str_of(o, "family"),
            job: Job::parse(o.get("job")?.as_str()?)?,
            present: strs(o, "present")
                .iter()
                .filter_map(|j| Job::parse(j))
                .collect(),
            wait: o
                .get("wait")
                .and_then(JsValue::as_str)
                .and_then(WaitShape::parse),
            declared_delay_sec: num(o, "declaredDelaySec").unwrap_or(0.0),
            piped: flag("piped"),
            bg: flag("bg"),
            capped: flag("capped"),
        }))
    } else {
        None
    };
    Some(CallFact {
        base: base_of(o)?,
        seq: num(o, "seq")?,
        tool_use_id: opt_str(o, "toolUseId"),
        tool: str_of(o, "tool"),
        ts: opt_str(o, "ts"),
        dur_sec: num(o, "durSec"),
        err: flag("err"),
        failed: flag("failed"),
        fail_reason: opt_str(o, "failReason"),
        detector: opt_str(o, "detector"),
        benign: opt_str(o, "benign"),
        sig: opt_str(o, "sig"),
        arg: opt_str(o, "arg"),
        out_chars: num(o, "outChars").unwrap_or(0.0),
        side: flag("side"),
        req: opt_str(o, "req"),
        ctx_tok: num(o, "ctxTok"),
        cause: o
            .get("cause")
            .and_then(JsValue::as_str)
            .and_then(CauseId::parse),
        cause_arg: opt_str(o, "causeArg"),
        focus: opt_str(o, "focus"),
        bash,
    })
}

/// A shard line that is not a fact row.
struct Corrupt;

/// One shard line: a fact, `None` for a row of another kind.
fn fact_of(v: &JsValue) -> Result<Option<Fact>, Corrupt> {
    fact_or_skip(v).ok_or(Corrupt)
}

#[expect(
    clippy::option_option,
    reason = "outer None is a corrupt line, inner None a foreign row"
)]
fn fact_or_skip(v: &JsValue) -> Option<Option<Fact>> {
    let o = v.as_obj()?;
    let row = o.get("row").and_then(JsValue::as_str).unwrap_or("");
    let n = |k: &str| num(o, k).unwrap_or(0.0);
    Some(match row {
        "session" => Some(Fact::Session(SessionFact {
            base: base_of(o)?,
            started_at: opt_str(o, "startedAt"),
            ended_at: opt_str(o, "endedAt"),
            title: opt_str(o, "title"),
            cwd: opt_str(o, "cwd"),
            source: opt_str(o, "source"),
            models: strs(o, "models"),
            turns: n("turns"),
            user_messages: n("userMessages"),
            tool_calls: n("toolCalls"),
            input_tokens: n("inputTokens"),
            output_tokens: n("outputTokens"),
            cache_read_tokens: n("cacheReadTokens"),
            cache_creation_tokens: n("cacheCreationTokens"),
            max_context_tokens: n("maxContextTokens"),
            first_turn_cache_creation: n("firstTurnCacheCreation"),
        })),
        "tool" => Some(Fact::Call(call_of(o, false)?)),
        "bash" => Some(Fact::Call(call_of(o, true)?)),
        "usage" => Some(Fact::Usage(UsageFact {
            sid: str_of(o, "sid"),
            agent: Agent::parse(o.get("agent")?.as_str()?)?,
            ts: opt_str(o, "ts"),
            req: str_of(o, "req"),
            model: str_of(o, "model"),
            input_tokens: n("inputTokens"),
            output_tokens: n("outputTokens"),
            cache_read_tokens: n("cacheReadTokens"),
            cache_creation_tokens: n("cacheCreationTokens"),
        })),
        _ => None,
    })
}

/// Facts of one shard; `None` when any line is corrupt.
fn parse_shard(raw: &str) -> Option<Vec<Fact>> {
    let mut out = Vec::new();
    for line in raw.split('\n').filter(|l| !l.is_empty()) {
        if let Some(f) = fact_of(&JsValue::parse(line)?).ok()? {
            out.push(f);
        }
    }
    Some(out)
}

fn entry_of(v: &JsValue) -> Option<CacheEntry> {
    let o = v.as_obj()?;
    Some(CacheEntry {
        path: str_of(o, "path"),
        mtime_ms: num(o, "mtimeMs")?,
        size: num(o, "size")?,
        shard: str_of(o, "shard"),
        rows: num(o, "rows").unwrap_or(0.0),
        last_fact_ms: num(o, "lastFactMs"),
        extractor_version: num(o, "extractorVersion").unwrap_or(0.0),
    })
}

/// Held across each index rewrite in this process, so passes running side
/// by side never drop each other's entries.
static INDEX_LOCK: Mutex<()> = Mutex::new(());

/// Write `body` to `path` through a temp file and a rename. The temp name
/// is unique, so two writers of one path never mix their bytes.
fn write_atomic(path: &str, body: &str) -> std::io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    if let Some(dir) = Path::new(path).parent() {
        fs::create_dir_all(dir)?;
    }
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let tmp = format!("{path}.{}-{n}.tmp", std::process::id());
    let written = fs::write(&tmp, body).and_then(|()| fs::rename(&tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

/// `$PEEKTRACE_DIR/stats-rs`, else `~/.peektrace/stats-rs`. Separate from
/// the TS `stats/` dir, so the two never read each other's shards.
pub fn cache_root(env: &RootsEnv) -> String {
    let dir = env
        .peektrace_dir
        .clone()
        .unwrap_or_else(|| env.style.join(&[&env.home, ".peektrace"]));
    env.style.join(&[&dir, "stats-rs"])
}

impl StatsCache {
    /// The cache under `root`; the ledger sits beside it.
    pub fn new(root: String) -> Self {
        let parent = Path::new(&root)
            .parent()
            .map_or_else(String::new, |p| p.to_string_lossy().into_owned());
        Self {
            index_path: format!("{root}/index.json"),
            shards_dir: format!("{root}/shards"),
            lines_path: format!("{parent}/rule-lines.json"),
            root,
        }
    }

    pub fn load(&self) -> IndexMap<String, CacheEntry> {
        let parsed = fs::read(&self.index_path)
            .ok()
            .and_then(|b| JsValue::parse(&String::from_utf8_lossy(&b)));
        let Some(o) = parsed.as_ref().and_then(JsValue::as_obj) else {
            return IndexMap::new();
        };
        if num(o, "version") != Some(INDEX_VERSION) {
            return IndexMap::new();
        }
        o.get("entries")
            .and_then(JsValue::as_obj)
            .map(|e| {
                e.entries()
                    .filter_map(|(k, v)| entry_of(v).map(|en| (k.to_owned(), en)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Facts of one cached transcript; `None` on a missing or corrupt shard.
    pub fn read(&self, entry: &CacheEntry) -> Option<Vec<Fact>> {
        let raw = fs::read(format!("{}/{}", self.shards_dir, entry.shard)).ok()?;
        parse_shard(&String::from_utf8_lossy(&raw))
    }

    /// Write one transcript's facts; `None` when it is not cacheable or the write fails.
    #[expect(clippy::cast_precision_loss)]
    pub fn write(
        &self,
        path: &str,
        stat: FileStat,
        facts: &[Fact],
        local: &dyn LocalZone,
    ) -> Option<CacheEntry> {
        if !is_cacheable(stat) {
            return None;
        }
        let shard = format!("{}.jsonl", &sha1_hex(path)[..SHARD_HASH_LEN]);
        let body = facts.iter().map(fact_json).collect::<Vec<_>>().join("\n");
        write_atomic(&format!("{}/{shard}", self.shards_dir), &body).ok()?;
        Some(CacheEntry {
            path: path.to_owned(),
            mtime_ms: stat.mtime_ms,
            size: stat.size,
            shard,
            rows: facts.len() as f64,
            last_fact_ms: last_fact_ms_of(facts, local),
            extractor_version: EXTRACTOR_VERSION,
        })
    }

    /// Replace the index with `entries`. Errors are ignored: the next pass re-extracts.
    pub fn commit(&self, entries: &[CacheEntry]) {
        let _held = INDEX_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        self.write_index(entries);
    }

    /// Rewrite the index from its current contents: `next` returns the
    /// entries to write, or `None` to keep it. No other index write in this
    /// process runs in between.
    pub fn update(
        &self,
        next: impl FnOnce(&IndexMap<String, CacheEntry>) -> Option<Vec<CacheEntry>>,
    ) {
        let _held = INDEX_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(entries) = next(&self.load()) {
            self.write_index(&entries);
        }
    }

    fn write_index(&self, entries: &[CacheEntry]) {
        let mut w = Writer::new(false);
        w.begin_obj();
        w.key("version").num(INDEX_VERSION);
        w.key("entries").begin_obj();
        for e in entries {
            w.key_text(&e.path).begin_obj();
            w.key("path").text(&e.path);
            w.key("mtimeMs").num(e.mtime_ms);
            w.key("size").num(e.size);
            w.key("shard").str(&e.shard);
            w.key("rows").num(e.rows);
            match e.last_fact_ms {
                Some(ms) => w.key("lastFactMs").num(ms),
                None => w.key("lastFactMs").null(),
            }
            w.key("extractorVersion").num(e.extractor_version);
            w.end_obj();
        }
        w.end_obj();
        w.end_obj();
        let _ = write_atomic(&self.index_path, &w.finish());
    }

    /// Delete every shard not named in `keep`.
    pub fn prune(&self, keep: &std::collections::HashSet<String>) {
        let Ok(read) = fs::read_dir(&self.shards_dir) else {
            return;
        };
        for e in read.filter_map(Result::ok) {
            let Ok(name) = e.file_name().into_string() else {
                continue;
            };
            #[expect(
                clippy::case_sensitive_file_extension_comparisons,
                reason = "shard names are written lowercase"
            )]
            let shard = name.ends_with(".jsonl");
            if shard && !keep.contains(&name) {
                let _ = fs::remove_file(e.path());
            }
        }
    }

    /// Drop the whole cache dir.
    pub fn clear(&self) {
        let _ = fs::remove_dir_all(&self.root);
    }

    pub fn load_lines(&self) -> LineLedger {
        let parsed = fs::read(&self.lines_path)
            .ok()
            .and_then(|b| JsValue::parse(&String::from_utf8_lossy(&b)));
        let Some(o) = parsed.as_ref().and_then(JsValue::as_obj) else {
            return LineLedger::default();
        };
        let files = o.get("files").and_then(JsValue::as_obj);
        match files {
            Some(files) if num(o, "version") == Some(LEDGER_VERSION) => LineLedger {
                files: files
                    .entries()
                    .map(|(path, lines)| {
                        let lines = lines
                            .as_obj()
                            .map(|l| {
                                l.entries()
                                    .map(|(k, v)| (k.to_owned(), v.as_num()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        (path.to_owned(), lines)
                    })
                    .collect(),
            },
            _ => LineLedger::default(),
        }
    }

    pub fn commit_lines(&self, ledger: &LineLedger) {
        let mut w = Writer::new(false);
        w.begin_obj();
        w.key("version").num(LEDGER_VERSION);
        w.key("files").begin_obj();
        for (path, lines) in &ledger.files {
            w.key_text(path).begin_obj();
            for (k, v) in lines {
                match v {
                    Some(ms) => w.key_text(k).num(*ms),
                    None => w.key_text(k).null(),
                }
            }
            w.end_obj();
        }
        w.end_obj();
        w.end_obj();
        let _ = write_atomic(&self.lines_path, &w.finish());
    }
}
