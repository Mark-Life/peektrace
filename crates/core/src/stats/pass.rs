//! One pass over the corpus: list transcripts, take each one's facts from
//! the cache or extract them, and fold the in-window facts into the sinks
//! in path order.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::UNIX_EPOCH;

use indexmap::IndexMap;
use rayon::prelude::*;
use tracing::{field, info_span};

use super::cache::{
    CacheEntry, FileStat, StatsCache, fact_ms, is_fresh, is_past_window, merge_index,
};
use super::extract::{ExtractInput, extract_facts};
use super::rank::uniq_sorted;
use super::redact::Redactor;
use super::schema::Fact;
use super::walk::{TranscriptRef, list_corpus};
use crate::claude::{ParseArgs, parse_claude_session};
use crate::codex::parse_codex_session;
use crate::js::time::LocalZone;
use crate::listing::{AgentRoots, default_root};
use crate::model::Agent;
use crate::opencode::{Reader, Store, parse_opencode_session};
use crate::pi::parse_pi_session;
use crate::sessions::model::ParsedSession;
use crate::sessions::resolve::read_text;

/// Files whose facts are held in memory at once before they are folded.
const CHUNK: usize = 512;

/// What a pass reads from the environment.
pub struct PassEnv<'a> {
    pub roots: &'a AgentRoots,
    pub cache: &'a StatsCache,
    pub local: &'a (dyn LocalZone + Sync),
    /// Told `(done, total)` after each file is folded.
    pub progress: Option<&'a dyn Fn(usize, usize)>,
}

/// The agents a request covers, in matrix order.
pub fn scope_of(requested: Option<&[Agent]>) -> Vec<Agent> {
    Agent::ALL
        .into_iter()
        .filter(|a| requested.is_none_or(|r| r.contains(a)))
        .collect()
}

/// True when `scope` holds every agent.
pub fn is_full_scope(scope: &[Agent]) -> bool {
    scope.len() == Agent::ALL.len()
}

fn parse_claude(text: &str, r: &TranscriptRef) -> ParsedSession {
    parse_claude_session(&ParseArgs {
        text,
        path: &r.path,
        session_id: &r.id,
        include_sidechain_turns: r.kind != super::schema::Kind::Main,
        skip_system_prompt: true,
    })
}

/// Load and parse one transcript; `None` when a text transcript cannot be
/// read. An `OpenCode` session no store holds parses as an empty session.
fn parse_ref(r: &TranscriptRef, opencode: &mut Reader<'_>) -> Option<ParsedSession> {
    let parse_text: fn(&str, &TranscriptRef) -> ParsedSession = match r.agent {
        Agent::Claude => parse_claude,
        Agent::Codex => |text, r| parse_codex_session(text, &r.path, &r.id),
        Agent::Pi => |text, r| parse_pi_session(text, &r.path, &r.id),
        Agent::Opencode => {
            let session = {
                let _span = info_span!(
                    "AgentRegistry.loadTranscript",
                    agent = Agent::Opencode.as_str()
                )
                .entered();
                opencode.load(&r.path, &r.id)
            };
            return Some(parse_opencode_session(session.as_ref(), &r.path, &r.id));
        }
    };
    load_transcript(r).map(|text| parse_text(&text, r))
}

/// What a pass folds and over which window.
pub struct PassArgs<'a> {
    pub scope: &'a [Agent],
    pub force: bool,
    pub from_ms: f64,
    pub to_ms: f64,
    pub only: Option<&'a (dyn Fn(&TranscriptRef) -> bool + Sync)>,
}

/// What one pass produced besides the sinks' state.
#[derive(Debug, Default)]
pub struct PassResult {
    pub transcripts: usize,
    pub extracted: usize,
    pub cached: usize,
    pub facts: usize,
    /// Every walked transcript's path, by session id (last wins).
    pub paths: HashMap<String, String>,
    pub skipped: Vec<String>,
}

struct FileResult {
    cached: bool,
    entry: Option<CacheEntry>,
    facts: Vec<Fact>,
    skipped: bool,
}

#[expect(clippy::cast_precision_loss)]
fn stat_of(path: &str) -> FileStat {
    fs::metadata(path).map_or_else(
        |_| FileStat::default(),
        |m| FileStat {
            size: m.len() as f64,
            mtime_ms: m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0.0, |d| d.as_millis() as f64),
        },
    )
}

fn load_transcript(r: &TranscriptRef) -> Option<String> {
    let span = info_span!(
        "AgentRegistry.loadTranscript",
        agent = r.agent.as_str(),
        error = field::Empty
    );
    let _e = span.enter();
    read_text(&r.path)
        .map_err(|e| span.record("error", field::display(&e)))
        .ok()
}

fn facts_for(
    cache: &StatsCache,
    local: &(dyn LocalZone + Sync),
    index: &IndexMap<String, CacheEntry>,
    r: &TranscriptRef,
    args: &PassArgs<'_>,
    opencode: &mut Reader<'_>,
) -> FileResult {
    let stat = stat_of(&r.path);
    if let Some(entry) = index
        .get(&r.path)
        .filter(|e| !args.force && is_fresh(e, &r.path, stat))
    {
        if is_past_window(entry, args.from_ms) {
            return FileResult {
                cached: true,
                entry: Some(entry.clone()),
                facts: Vec::new(),
                skipped: false,
            };
        }
        if let Some(facts) = cache.read(entry) {
            return FileResult {
                cached: true,
                entry: Some(entry.clone()),
                facts,
                skipped: false,
            };
        }
    }
    let skipped = FileResult {
        cached: false,
        entry: None,
        facts: Vec::new(),
        skipped: true,
    };
    let Some(parsed) = parse_ref(r, opencode) else {
        return skipped;
    };
    let project = if r.slug.is_empty() {
        encode_slug(parsed.cwd.as_deref().unwrap_or(""))
    } else {
        r.slug.clone()
    };
    let redact = Redactor::new(true);
    let facts = extract_facts(&ExtractInput {
        r#ref: r,
        parsed: &parsed,
        project: &project,
        redact: &redact,
        local,
    });
    let entry = cache.write(&r.path, stat, &facts, local);
    FileResult {
        cached: false,
        entry,
        facts,
        skipped: false,
    }
}

fn in_window(f: &Fact, from_ms: f64, to_ms: f64, local: &dyn LocalZone) -> bool {
    fact_ms(f, local).is_none_or(|t| t >= from_ms && t <= to_ms)
}

/// A project key from a cwd: every `/` and `.` becomes `-`.
fn encode_slug(cwd: &str) -> String {
    cwd.replace(['/', '.'], "-")
}

/// The transcripts a pass folds: the corpus, narrowed by `args.only`.
pub fn pass_refs(env: &PassEnv<'_>, args: &PassArgs<'_>) -> Vec<TranscriptRef> {
    let all = list_corpus(args.scope, env.roots);
    match args.only {
        Some(keep) => all.into_iter().filter(|r| keep(r)).collect(),
        None => all,
    }
}

/// Walk the corpus once, folding every in-window fact into `sink`.
pub fn run_pass(env: &PassEnv<'_>, args: &PassArgs<'_>, sink: &mut dyn FnMut(&Fact)) -> PassResult {
    fold_refs(env, args, &pass_refs(env, args), sink)
}

/// True when the index already holds exactly `entries`, in order. An empty
/// one is always written, so a fresh or outdated index gets replaced.
fn index_unchanged(index: &IndexMap<String, CacheEntry>, entries: &[CacheEntry]) -> bool {
    !index.is_empty()
        && index.len() == entries.len()
        && index.values().zip(entries).all(|(a, b)| a == b)
}

/// Fold `refs` (from `pass_refs`) in path order. Files are read one chunk
/// ahead on the rayon pool while the caller's thread folds the last chunk.
pub fn fold_refs(
    env: &PassEnv<'_>,
    args: &PassArgs<'_>,
    refs: &[TranscriptRef],
    sink: &mut dyn FnMut(&Fact),
) -> PassResult {
    let index = env.cache.load();
    let (cache, local) = (env.cache, env.local);
    let opencode_root = default_root(&env.roots.opencode);
    let opencode = &Store::new(
        refs.iter()
            .filter(|r| r.agent == Agent::Opencode)
            .map(|r| r.path.as_str()),
        &opencode_root,
    );
    let mut out = PassResult {
        transcripts: refs.len(),
        ..PassResult::default()
    };
    let mut entries: Vec<CacheEntry> = Vec::new();
    let mut skipped: Vec<&str> = Vec::new();
    let mut done = 0;
    {
        let _span = info_span!("Stats.foldCorpus", transcripts = refs.len()).entered();
        std::thread::scope(|scope| {
            let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<FileResult>>(0);
            let index = &index;
            scope.spawn(move || {
                for chunk in refs.chunks(CHUNK) {
                    let results: Vec<FileResult> = chunk
                        .par_iter()
                        .map_init(
                            || opencode.reader(),
                            |reader, r| facts_for(cache, local, index, r, args, reader),
                        )
                        .collect();
                    if tx.send(results).is_err() {
                        return;
                    }
                }
            });
            for (chunk, results) in refs.chunks(CHUNK).zip(rx) {
                for (r, res) in chunk.iter().zip(results) {
                    done += 1;
                    if let Some(p) = env.progress {
                        p(done, refs.len());
                    }
                    if res.skipped {
                        skipped.push(&r.path);
                        continue;
                    }
                    entries.extend(res.entry);
                    if res.cached {
                        out.cached += 1;
                    } else {
                        out.extracted += 1;
                    }
                    for f in res
                        .facts
                        .iter()
                        .filter(|f| in_window(f, args.from_ms, args.to_ms, env.local))
                    {
                        if !matches!(f, Fact::Usage(_)) {
                            out.facts += 1;
                        }
                        sink(f);
                    }
                }
            }
        });
    }
    if is_full_scope(args.scope) && args.only.is_none() {
        if !index_unchanged(&index, &entries) {
            env.cache.commit(&entries);
            env.cache.prune(
                &entries
                    .iter()
                    .map(|e| e.shard.clone())
                    .collect::<HashSet<_>>(),
            );
        }
    } else {
        let scanned: Vec<String> = refs.iter().map(|r| r.path.clone()).collect();
        let next = merge_index(&index, &entries, &scanned);
        if !index_unchanged(&index, &next) {
            env.cache.commit(&next);
        }
    }
    out.paths = refs
        .iter()
        .map(|r| (r.id.clone(), r.path.clone()))
        .collect();
    out.skipped = uniq_sorted(skipped);
    out
}
