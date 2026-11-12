//! The memory files each agent loads for a project (CLAUDE.md, AGENTS.md,
//! rules dirs, `@imports`), read for the rule check.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::time::UNIX_EPOCH;

use tracing::info_span;

use super::detectors::rules::{CodexCover, MemoryDoc, RuleCover};
use super::memory_paths::{codex_dirs_of, imports_of};
use super::rank::cmp_str;
use crate::js::regex::LazyRe;
use crate::js_re;
use crate::listing::AgentRoots;
use crate::model::Agent;
use crate::roots::Source;
use crate::sessions::resolve::{exists, read_text};

static SLASHES: LazyRe = js_re!(r"[\\/]+", "g");
static AGENT_HOME: LazyRe = js_re!(r"^(.+)\/\.(?:claude|codex|pi)\/?$");
static MARKDOWN: LazyRe = js_re!(r"\.md$", "i");
const IMPORT_DEPTH: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pick {
    All,
    First,
}

struct FileSet {
    pick: Pick,
    names: &'static [&'static str],
}

const CODEX_FILES: FileSet = FileSet {
    pick: Pick::First,
    names: &["AGENTS.override.md", "AGENTS.md"],
};
const PI_FILES: FileSet = FileSet {
    pick: Pick::First,
    names: &[
        "AGENTS.override.md",
        "AGENTS.md",
        "AGENTS.MD",
        "CLAUDE.md",
        "CLAUDE.MD",
    ],
};

const fn project_files(agent: Agent) -> FileSet {
    match agent {
        Agent::Claude => FileSet {
            pick: Pick::All,
            names: &["CLAUDE.md", "CLAUDE.local.md", ".claude/CLAUDE.md"],
        },
        Agent::Codex => CODEX_FILES,
        Agent::Pi => PI_FILES,
        Agent::Opencode => FileSet {
            pick: Pick::First,
            names: &["AGENTS.md", "CLAUDE.md"],
        },
    }
}

fn slashed(path: &str) -> String {
    SLASHES.replace_all_str(path, "/")
}

pub(super) fn join(parts: &[&str]) -> String {
    slashed(&parts.join("/"))
}

pub(super) fn dirname(path: &str) -> &str {
    match path.rfind('/') {
        Some(at) if at > 0 => &path[..at],
        _ => "/",
    }
}

/// `dir` and each ancestor, nearest first, up to the user's home when under it.
pub(super) fn ancestors(dir: &str, user_home: Option<&str>) -> Vec<String> {
    let s = slashed(dir);
    let start = s.strip_suffix('/').unwrap_or(&s).to_owned();
    if start.is_empty() {
        return if s == "/" { vec!["/".into()] } else { vec![] };
    }
    let stop = user_home.filter(|h| start.starts_with(&format!("{h}/")));
    let mut out = Vec::new();
    let mut d = start;
    loop {
        out.push(d.clone());
        let at = d.rfind('/');
        if Some(d.as_str()) == stop || d == "/" || at.is_none() {
            return out;
        }
        d = match at {
            Some(0) => "/".into(),
            Some(a) => d[..a].to_owned(),
            None => return out,
        };
    }
}

fn tilde(path: &str, user_home: Option<&str>) -> String {
    match user_home {
        Some(h) if path == h || path.starts_with(&format!("{h}/")) => {
            format!("~{}", &path[h.len()..])
        }
        _ => path.to_owned(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CandKind {
    File,
    Rules,
}

#[derive(Clone)]
struct Candidate {
    cover: RuleCover,
    depth: usize,
    group: String,
    kind: CandKind,
    path: String,
}

fn cover_key(c: &RuleCover) -> String {
    [
        c.agent.as_str().to_owned(),
        c.dir.clone().unwrap_or_default(),
        c.source.as_ref().map(|s| s.0.clone()).unwrap_or_default(),
        c.codex.as_ref().map_or_else(String::new, |x| {
            format!("{}\0{}", x.exact, x.stops.join("\0"))
        }),
    ]
    .join("\0")
}

fn file_set_in(
    dir: &str,
    files: &FileSet,
    cover: &RuleCover,
    skip: Option<&str>,
) -> Vec<Candidate> {
    let first = format!("{dir}\0{}", cover_key(cover));
    files
        .names
        .iter()
        .filter(|n| Some(**n) != skip)
        .map(|name| {
            let path = join(&[dir, name]);
            Candidate {
                group: if files.pick == Pick::First {
                    first.clone()
                } else {
                    format!("{path}\0{}", cover_key(cover))
                },
                path,
                cover: cover.clone(),
                depth: 0,
                kind: CandKind::File,
            }
        })
        .collect()
}

fn rules_dir_in(dir: &str, cover: &RuleCover) -> Candidate {
    let path = join(&[dir, "rules"]);
    Candidate {
        group: format!("{path}\0{}", cover_key(cover)),
        path,
        cover: cover.clone(),
        depth: 0,
        kind: CandKind::Rules,
    }
}

fn plain_cover(agent: Agent, dir: Option<String>) -> RuleCover {
    RuleCover {
        agent,
        codex: None,
        dir,
        source: None,
    }
}

/// Where each agent keeps its own config.
pub struct AgentHomes<'a> {
    pub claude: &'a [Source],
    pub codex_home: String,
    pub pi_home: String,
}

impl<'a> AgentHomes<'a> {
    /// Each agent's default root home, as TS `agents.roots(agent).home`.
    pub fn of(roots: &'a AgentRoots) -> Self {
        let home = |agent| {
            roots
                .of(agent)
                .first()
                .map_or_else(String::new, |s: &Source| s.home.clone())
        };
        Self {
            claude: &roots.claude,
            codex_home: home(Agent::Codex),
            pi_home: home(Agent::Pi),
        }
    }
}

fn global_candidates(
    homes: &AgentHomes<'_>,
    scope: &[Agent],
    user_home: Option<&str>,
) -> Vec<Candidate> {
    let mut out = Vec::new();
    if scope.contains(&Agent::Claude) {
        let multi = homes.claude.len() > 1;
        for (i, source) in homes.claude.iter().enumerate() {
            let cover = RuleCover {
                source: multi.then(|| (source.id.clone(), i == 0)),
                ..plain_cover(Agent::Claude, None)
            };
            let set = FileSet {
                pick: Pick::All,
                names: &["CLAUDE.md"],
            };
            out.extend(file_set_in(&source.home, &set, &cover, None));
            out.push(rules_dir_in(&source.home, &cover));
        }
    }
    if scope.contains(&Agent::Codex) {
        out.extend(file_set_in(
            &homes.codex_home,
            &CODEX_FILES,
            &plain_cover(Agent::Codex, None),
            None,
        ));
    }
    if scope.contains(&Agent::Pi) {
        out.extend(file_set_in(
            &join(&[&homes.pi_home, "agent"]),
            &PI_FILES,
            &plain_cover(Agent::Pi, None),
            None,
        ));
    }
    if let Some(h) = user_home.filter(|_| scope.contains(&Agent::Opencode)) {
        let set = FileSet {
            pick: Pick::First,
            names: &[".config/opencode/AGENTS.md", ".claude/CLAUDE.md"],
        };
        out.extend(file_set_in(
            h,
            &set,
            &plain_cover(Agent::Opencode, None),
            None,
        ));
    }
    out
}

fn candidates_of(
    homes: &AgentHomes<'_>,
    scope: &[Agent],
    project_dirs: &[String],
    user_home: Option<&str>,
    codex_dirs: &HashMap<String, CodexCover>,
) -> Vec<Candidate> {
    let mut out = global_candidates(homes, scope, user_home);
    let mut dirs: Vec<String> = project_dirs
        .iter()
        .flat_map(|d| ancestors(d, user_home))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    dirs.sort_by(|a, b| cmp_str(a, b));
    for dir in &dirs {
        for &agent in scope {
            let codex = if agent == Agent::Codex {
                match codex_dirs.get(dir) {
                    Some(c) => Some(c.clone()),
                    None => continue,
                }
            } else {
                None
            };
            let at_home = Some(dir.as_str()) == user_home;
            let cover = RuleCover {
                codex,
                ..plain_cover(agent, Some(dir.clone()))
            };
            out.extend(file_set_in(
                dir,
                &project_files(agent),
                &cover,
                at_home.then_some(".claude/CLAUDE.md"),
            ));
            if agent == Agent::Claude && !at_home {
                out.push(rules_dir_in(
                    &join(&[dir, ".claude"]),
                    &plain_cover(agent, Some(dir.clone())),
                ));
            }
        }
    }
    out
}

fn user_home_of(homes: &AgentHomes<'_>) -> Option<String> {
    let claude = homes
        .claude
        .first()
        .map(|s| s.home.clone())
        .unwrap_or_default();
    [claude, homes.codex_home.clone(), homes.pi_home.clone()]
        .iter()
        .find_map(|h| {
            AGENT_HOME
                .captures(&slashed(h))
                .and_then(|c| c.get(1))
                .map(str::to_owned)
        })
}

/// Every file under `dir`, relative, recursively.
fn walk_names(dir: &str, prefix: &str, out: &mut Vec<String>, depth: usize) -> bool {
    let Ok(read) = fs::read_dir(dir) else {
        return false;
    };
    for e in read.filter_map(Result::ok) {
        let Ok(name) = e.file_name().into_string() else {
            continue;
        };
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        out.push(rel.clone());
        if depth < 32 && fs::metadata(e.path()).is_ok_and(|m| m.is_dir()) {
            walk_names(&format!("{dir}/{name}"), &rel, out, depth + 1);
        }
    }
    true
}

fn rules_of(dir: &Candidate) -> Vec<Candidate> {
    let mut names = Vec::new();
    if !walk_names(&dir.path, "", &mut names, 0) {
        return Vec::new();
    }
    let mut names: Vec<String> = names
        .into_iter()
        .filter(|n| MARKDOWN.is_match(n))
        .map(|n| slashed(&n))
        .collect();
    names.sort_by(|a, b| cmp_str(a, b));
    names
        .into_iter()
        .map(|name| {
            let path = join(&[&dir.path, &name]);
            Candidate {
                group: format!("{path}\0{}", cover_key(&dir.cover)),
                path,
                kind: CandKind::File,
                ..dir.clone()
            }
        })
        .collect()
}

struct ContextText {
    real_path: String,
    mtime_ms: Option<f64>,
    text: String,
}

#[expect(clippy::cast_precision_loss)]
fn read_context_text(path: &str) -> Option<ContextText> {
    if !exists(path) {
        return None;
    }
    let real_path = crate::paths::real_path(path);
    let meta = fs::metadata(&real_path).ok()?;
    let text = read_text(&real_path).ok()?;
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64);
    Some(ContextText {
        real_path,
        mtime_ms,
        text,
    })
}

static NEWLINE: LazyRe = js_re!(r"\r?\n");

/// Read every memory file in scope; a missing file is simply not there.
pub fn read_memory_docs(
    homes: &AgentHomes<'_>,
    scope: &[Agent],
    project_dirs: &[String],
) -> Vec<MemoryDoc> {
    let _span = info_span!("Stats.memoryDocs").entered();
    let user_home = user_home_of(homes);
    let uh = user_home.as_deref();
    let codex_dirs = if scope.contains(&Agent::Codex) {
        codex_dirs_of(project_dirs, uh)
    } else {
        HashMap::new()
    };
    let mut queue: VecDeque<Candidate> =
        candidates_of(homes, scope, project_dirs, uh, &codex_dirs).into();
    let mut docs = Vec::new();
    let mut seen = HashSet::new();
    let mut settled = HashSet::new();
    while let Some(c) = queue.pop_front() {
        if settled.contains(&c.group) {
            continue;
        }
        if c.kind == CandKind::Rules {
            for r in rules_of(&c).into_iter().rev() {
                queue.push_front(r);
            }
            continue;
        }
        let Some(read) = read_context_text(&c.path) else {
            continue;
        };
        settled.insert(c.group.clone());
        let real_path = slashed(&read.real_path);
        if !seen.insert(format!("{real_path}\0{}", cover_key(&c.cover))) {
            continue;
        }
        let lines: Vec<String> = NEWLINE
            .split(&read.text)
            .into_iter()
            .map(str::to_owned)
            .collect();
        let shown = tilde(&c.path, uh);
        let display_path = if real_path == c.path {
            shown
        } else {
            format!("{shown} → {}", tilde(&real_path, uh))
        };
        if c.depth < IMPORT_DEPTH {
            let imports = imports_of(&lines, &real_path, uh);
            for path in imports.into_iter().rev() {
                queue.push_front(Candidate {
                    group: format!("{path}\0{}", cover_key(&c.cover)),
                    path,
                    depth: c.depth + 1,
                    kind: CandKind::File,
                    cover: c.cover.clone(),
                });
            }
        }
        docs.push(MemoryDoc {
            cover: c.cover,
            display_path,
            lines,
            mtime_ms: read.mtime_ms,
            real_path,
        });
    }
    docs
}
