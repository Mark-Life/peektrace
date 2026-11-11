//! One pass over the corpus: list transcripts, take each one's facts from
//! the cache or extract them, and fold the in-window facts into the sinks
//! in path order.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::UNIX_EPOCH;

use indexmap::IndexMap;
use rayon::prelude::*;
use tracing::{field, info_span};

use super::cache::{CacheEntry, FileStat, StatsCache, fact_ms, is_fresh, is_past_window, merge_index};
use super::extract::{ExtractInput, extract_facts};
use super::rank::uniq_sorted;
use super::redact::Redactor;
use super::schema::Fact;
use super::walk::{TranscriptRef, list_corpus};
use crate::claude::{ParseArgs, parse_claude_session};
use crate::js::time::LocalZone;
use crate::model::Agent;
use crate::roots::Source;
use crate::sessions::model::ParsedSession;
use crate::sessions::resolve::read_text;

/// Files whose facts are held in memory at once before they are folded.
const CHUNK: usize = 64;

/// What a pass reads from the environment.
pub struct PassEnv<'a> {
    pub claude: &'a [Source],
    pub cache: &'a StatsCache,
    pub local: &'a (dyn LocalZone + Sync),
}

/// The agents a request covers that this build can extract, in matrix order.
pub fn scope_of(requested: Option<&[Agent]>) -> Vec<Agent> {
    Agent::ALL
        .into_iter()
        .filter(|a| requested.is_none_or(|r| r.contains(a)) && parse_for(*a).is_some())
        .collect()
}

/// True when `scope` holds every agent this build can extract.
pub fn is_full_scope(scope: &[Agent]) -> bool {
    scope.len() == scope_of(None).len()
}

/// What one agent's parser reads.
pub struct ParseInput<'a> {
    pub text: &'a str,
    pub r#ref: &'a TranscriptRef,
}

pub type ParseFn = fn(&ParseInput<'_>) -> ParsedSession;

fn parse_claude(input: &ParseInput<'_>) -> ParsedSession {
    parse_claude_session(&ParseArgs {
        text: input.text,
        path: &input.r#ref.path,
        session_id: &input.r#ref.id,
        include_sidechain_turns: input.r#ref.kind != super::schema::Kind::Main,
        skip_system_prompt: true,
    })
}

/// The parser of an agent, one function per agent; `None` until it is ported.
pub fn parse_for(agent: Agent) -> Option<ParseFn> {
    match agent {
        Agent::Claude => Some(parse_claude),
        Agent::Codex | Agent::Pi | Agent::Opencode => None,
    }
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
    let span = info_span!("AgentRegistry.loadTranscript", agent = r.agent.as_str(), error = field::Empty);
    let _e = span.enter();
    read_text(&r.path)
        .map_err(|e| span.record("error", field::display(&e)))
        .ok()
}

fn facts_for(
    env: &PassEnv<'_>,
    index: &IndexMap<String, CacheEntry>,
    r: &TranscriptRef,
    args: &PassArgs<'_>,
) -> FileResult {
    let stat = stat_of(&r.path);
    if let Some(entry) = index.get(&r.path).filter(|e| !args.force && is_fresh(e, &r.path, stat)) {
        if is_past_window(entry, args.from_ms) {
            return FileResult { cached: true, entry: Some(entry.clone()), facts: Vec::new(), skipped: false };
        }
        if let Some(facts) = env.cache.read(entry) {
            return FileResult { cached: true, entry: Some(entry.clone()), facts, skipped: false };
        }
    }
    let skipped = FileResult { cached: false, entry: None, facts: Vec::new(), skipped: true };
    let Some(parse) = parse_for(r.agent) else { return skipped };
    let Some(text) = load_transcript(r) else { return skipped };
    let parsed = parse(&ParseInput { text: &text, r#ref: r });
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
        local: env.local,
    });
    let entry = env.cache.write(&r.path, stat, &facts, env.local);
    FileResult { cached: false, entry, facts, skipped: false }
}

fn in_window(f: &Fact, from_ms: f64, to_ms: f64, local: &dyn LocalZone) -> bool {
    fact_ms(f, local).is_none_or(|t| t >= from_ms && t <= to_ms)
}

/// A project key from a cwd: every `/` and `.` becomes `-`.
fn encode_slug(cwd: &str) -> String {
    cwd.replace(['/', '.'], "-")
}

/// Walk the corpus once, folding every in-window fact into `sink`.
pub fn run_pass(env: &PassEnv<'_>, args: &PassArgs<'_>, sink: &mut dyn FnMut(&Fact)) -> PassResult {
    let all = list_corpus(args.scope, env.claude);
    let refs: Vec<TranscriptRef> = match args.only {
        Some(keep) => all.into_iter().filter(|r| keep(r)).collect(),
        None => all,
    };
    let index = env.cache.load();
    let mut out = PassResult {
        transcripts: refs.len(),
        ..PassResult::default()
    };
    let mut entries: Vec<CacheEntry> = Vec::new();
    let mut skipped: Vec<&str> = Vec::new();
    {
        let _span = info_span!("Stats.foldCorpus", transcripts = refs.len()).entered();
        for chunk in refs.chunks(CHUNK) {
            let results: Vec<FileResult> = chunk.par_iter().map(|r| facts_for(env, &index, r, args)).collect();
            for (r, res) in chunk.iter().zip(results) {
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
                for f in res.facts.iter().filter(|f| in_window(f, args.from_ms, args.to_ms, env.local)) {
                    if !matches!(f, Fact::Usage(_)) {
                        out.facts += 1;
                    }
                    sink(f);
                }
            }
        }
    }
    if is_full_scope(args.scope) && args.only.is_none() {
        env.cache.commit(&entries);
        env.cache.prune(&entries.iter().map(|e| e.shard.clone()).collect::<HashSet<_>>());
    } else {
        let scanned: Vec<String> = refs.iter().map(|r| r.path.clone()).collect();
        env.cache.commit(&merge_index(&env.cache.load(), &entries, &scanned));
    }
    out.paths = refs.iter().map(|r| (r.id.clone(), r.path.clone())).collect();
    out.skipped = uniq_sorted(skipped);
    out
}
