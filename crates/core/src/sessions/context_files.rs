//! The instruction files Claude Code loads for a session, read from disk
//! now: global `CLAUDE.md` and rules, project `CLAUDE.md` / `AGENTS.md` /
//! `CLAUDE.local.md` and rules up the directory tree, and `MEMORY.md`.

use std::collections::HashSet;
use std::fs;

use tracing::info_span;

use super::model::{ContextScope, OnDiskContextFile};
use super::resolve::{exists, read_text, slash_join};
use crate::js::{num, text, units};
use crate::paths::posix;

const MAX_INDEX_LINES: usize = 200;
/// Claude Code cuts MEMORY.md past this many UTF-16 units, at a newline.
const MEMORY_MAX_CHARS: usize = 25_000;
const MAX_RULE_DEPTH: usize = 8;

const CLAUDE_FILES: [&str; 2] = ["CLAUDE.md", ".claude/CLAUDE.md"];
const AGENTS_FILES: [&str; 2] = ["AGENTS.md", ".claude/AGENTS.md"];
const LOCAL_FILE: &str = "CLAUDE.local.md";
const RULES_DIR: &str = ".claude/rules";

/// One context file as it sits on disk, symlinks followed.
struct ContextRead {
    bytes: f64,
    /// `None` when it exists but its text could not be read.
    text: Option<String>,
}

#[expect(clippy::cast_precision_loss, reason = "file sizes fit f64")]
fn read_context(path: &str) -> Option<ContextRead> {
    if !exists(path) {
        return None;
    }
    let real = fs::canonicalize(path)
        .map_or_else(|_| path.to_owned(), |p| p.to_string_lossy().into_owned());
    let meta = fs::metadata(&real).ok()?;
    Some(ContextRead {
        bytes: meta.len() as f64,
        text: read_text(&real).ok(),
    })
}

fn read_context_file(path: &str, label: &str, scope: ContextScope) -> Option<OnDiskContextFile> {
    let read = read_context(path)?;
    let tokens_est = match read.text.as_deref() {
        Some(t) if !t.is_empty() => units::est_tokens(t),
        _ => num::js_round(read.bytes / 4.0),
    };
    Some(OnDiskContextFile {
        label: label.to_owned(),
        path: path.to_owned(),
        bytes: read.bytes,
        tokens_est,
        scope,
    })
}

/// Split on commas outside `{…}`, as Claude Code splits a `paths` value.
fn split_globs(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth: i64 = 0;
    let mut part = String::new();
    for c in value.chars() {
        if c == ',' && depth == 0 {
            out.push(std::mem::take(&mut part));
        } else {
            depth += i64::from(c == '{');
            depth -= i64::from(c == '}');
            part.push(c);
        }
    }
    out.push(part);
    out
}

/// The frontmatter block: `^﻿?---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)`.
fn frontmatter(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let rest = rest.strip_prefix("---")?;
    let body = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))?;
    let base = text.len() - body.len();
    // The shortest block ending at a `\r?\n---` that closes a line or the text.
    let mut from = 0;
    while let Some(at) = body[from..].find("\n---") {
        let nl = from + at;
        let after = &body[nl + 4..];
        if after.is_empty() || after.starts_with('\n') || after.starts_with("\r\n") {
            let end = if body[..nl].ends_with('\r') {
                nl - 1
            } else {
                nl
            };
            return Some(&text[base..base + end]);
        }
        from = nl + 1;
    }
    None
}

/// `paths\s*:(.*)` at the start of a line: the inline value.
fn paths_key(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("paths")?;
    let rest = rest.trim_start_matches(text::is_js_whitespace);
    rest.strip_prefix(':')
        .filter(|v| !v.contains(['\r', '\u{2028}', '\u{2029}']))
}

/// `^\s+-\s*(.*)$`: a YAML list item.
fn list_item(line: &str) -> Option<&str> {
    let ws = text::is_js_whitespace;
    let trimmed = line.trim_start_matches(ws);
    if trimmed.len() == line.len() {
        return None;
    }
    let value = trimmed.strip_prefix('-')?.trim_start_matches(ws);
    (!value.contains(['\r', '\u{2028}', '\u{2029}'])).then_some(value)
}

/// `^(["'])(.*)\1$` unquoted.
fn unquote(g: &str) -> &str {
    for q in ['"', '\''] {
        if g.len() >= 2
            && let Some(inner) = g.strip_prefix(q).and_then(|r| r.strip_suffix(q))
        {
            return inner;
        }
    }
    g
}

/// The `paths` globs in a rule's frontmatter; empty when it has none.
pub fn rule_globs(text_in: &str) -> Vec<String> {
    let lines: Vec<&str> = frontmatter(text_in)
        .map(|b| {
            b.split('\n')
                .map(|l| l.strip_suffix('\r').unwrap_or(l))
                .collect()
        })
        .unwrap_or_default();
    let Some(at) = lines.iter().position(|l| paths_key(l).is_some()) else {
        return Vec::new();
    };
    let inline = paths_key(lines[at]).map_or("", text::trim);
    let raw: Vec<String> = if inline.is_empty() {
        lines[at + 1..]
            .iter()
            .map_while(|l| list_item(l))
            .map(str::to_owned)
            .collect()
    } else {
        let v = inline.strip_prefix('[').unwrap_or(inline);
        vec![v.strip_suffix(']').unwrap_or(v).to_owned()]
    };
    raw.iter()
        .flat_map(|r| split_globs(r))
        .map(|g| text::trim(unquote(text::trim(&g))).to_owned())
        .map(|g| g.strip_suffix("/**").map_or(g.clone(), str::to_owned))
        .filter(|g| !g.is_empty())
        .collect()
}

/// A rule with `paths` globs loads only when a matching file is touched.
pub fn is_conditional_rule(text: &str) -> bool {
    let globs = rule_globs(text);
    !globs.is_empty() && !globs.iter().all(|g| g == "**")
}

/// Unconditional `.md` rules under `dir`, subdirs included, in name order.
pub fn rule_paths(dir: &str) -> Vec<String> {
    #[expect(
        clippy::case_sensitive_file_extension_comparisons,
        reason = "Claude Code loads only lowercase `.md` rules"
    )]
    fn walk(dir: &str, depth: usize, seen: &mut HashSet<String>, out: &mut Vec<String>) {
        let real = fs::canonicalize(dir)
            .map_or_else(|_| dir.to_owned(), |p| p.to_string_lossy().into_owned());
        if depth > MAX_RULE_DEPTH || !seen.insert(real) {
            return;
        }
        let mut names: Vec<String> = fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort_by(|a, b| text::cmp_utf16(a, b));
        for name in names {
            let path = posix::join(&[dir, &name]);
            match fs::metadata(&path) {
                Ok(m) if m.is_dir() => walk(&path, depth + 1, seen, out),
                Ok(m)
                    if m.is_file()
                        && path.ends_with(".md")
                        && read_text(&path).is_ok_and(|t| !is_conditional_rule(&t)) =>
                {
                    out.push(path);
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, 0, &mut HashSet::new(), &mut out);
    out
}

/// `cwd` and every ancestor except `/`, farthest first.
pub fn project_dirs(cwd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut d = posix::resolve("/", cwd);
    while d != "/" {
        let parent = posix::dirname(&d);
        out.push(std::mem::replace(&mut d, parent));
    }
    out.reverse();
    out
}

/// The git checkout holding a dir: its root and its main checkout's root.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Checkout {
    main_root: String,
    root: String,
}

/// The first `gitdir:` value of a worktree's `.git` file.
fn gitdir_value(file: &str) -> Option<&str> {
    let mut start = 0;
    loop {
        let line_end = file[start..]
            .find(['\n', '\r', '\u{2028}', '\u{2029}'])
            .map_or(file.len(), |e| start + e);
        if let Some(rest) = file[start..].strip_prefix("gitdir:") {
            let value = rest.trim_start_matches(text::is_js_whitespace);
            let end = value
                .find(['\n', '\r', '\u{2028}', '\u{2029}'])
                .unwrap_or(value.len());
            let value = text::trim_end(&value[..end]);
            if !value.is_empty() {
                return Some(value);
            }
        }
        if line_end >= file.len() {
            return None;
        }
        start = line_end + file[line_end..].chars().next().map_or(1, char::len_utf8);
    }
}

/// Main checkout root named by a worktree's `.git` file, else `dir`.
fn main_root_of(dir: &str, git_file: &str) -> String {
    const MARK: &str = "/.git/worktrees/";
    let Some(raw) = gitdir_value(git_file) else {
        return dir.to_owned();
    };
    let resolved = posix::resolve(dir, raw);
    match resolved.rfind(MARK) {
        Some(at)
            if {
                let tail = &resolved[at + MARK.len()..];
                !tail.is_empty() && !tail.contains('/')
            } =>
        {
            resolved[..at].to_owned()
        }
        _ => dir.to_owned(),
    }
}

/// The nearest checkout among `dirs` (farthest first), or `None` outside git.
fn git_checkout(dirs: &[String]) -> Option<Checkout> {
    dirs.iter().rev().find_map(|dir| {
        let dot_git = posix::join(&[dir, ".git"]);
        let meta = fs::metadata(&dot_git).ok()?;
        if meta.is_dir() {
            return Some(Checkout {
                main_root: dir.clone(),
                root: dir.clone(),
            });
        }
        meta.is_file().then(|| Checkout {
            main_root: main_root_of(dir, &read_text(&dot_git).unwrap_or_default()),
            root: dir.clone(),
        })
    })
}

fn within(p: &str, root: &str) -> bool {
    p == root || p.strip_prefix(root).is_some_and(|r| r.starts_with('/'))
}

/// A dir of the main checkout outside the worktree nested in it: Claude Code
/// skips project files there (not `CLAUDE.local.md`).
fn outside_worktree(dir: &str, co: Option<&Checkout>) -> bool {
    co.is_some_and(|co| {
        co.main_root != co.root && within(dir, &co.main_root) && !within(dir, &co.root)
    })
}

/// Project instruction files for `cwd` in load order, and the root its
/// memory dir is keyed by.
fn project_context(cwd: &str, home: &str) -> (Vec<String>, String) {
    let dirs = project_dirs(cwd);
    let co = git_checkout(&dirs);
    let user_level = [
        posix::join(&[home, "CLAUDE.md"]),
        posix::join(&[home, "rules"]),
    ];
    let is_user = |p: &str| user_level.iter().any(|u| u == p);
    let project_files = |dir: &str, names: &[&str]| -> Vec<String> {
        if outside_worktree(dir, co.as_ref()) {
            Vec::new()
        } else {
            names.iter().map(|n| posix::join(&[dir, n])).collect()
        }
    };
    let is_claude_project = dirs
        .iter()
        .flat_map(|d| {
            project_files(d, &CLAUDE_FILES)
                .into_iter()
                .chain(std::iter::once(posix::join(&[d, LOCAL_FILE])))
        })
        .any(|p| !is_user(&p) && exists(&p));
    let names: &[&str] = if is_claude_project {
        &CLAUDE_FILES
    } else {
        &AGENTS_FILES
    };
    let mut paths = Vec::new();
    for dir in &dirs {
        let rules_dir = posix::join(&[dir, RULES_DIR]);
        paths.extend(project_files(dir, names));
        if !(outside_worktree(dir, co.as_ref()) || is_user(&rules_dir)) {
            paths.extend(rule_paths(&rules_dir));
        }
        paths.push(posix::join(&[dir, LOCAL_FILE]));
    }
    paths.retain(|p| !is_user(p));
    let memory_root = co.map_or_else(|| cwd.to_owned(), |c| c.main_root);
    (paths, memory_root)
}

/// Trim, keep 200 lines, then cut past 25,000 UTF-16 units at the last
/// newline before the limit.
pub fn cap_memory(text_in: &str) -> String {
    let trimmed = text::trim(text_in);
    let lines: Vec<&str> = trimmed.split('\n').collect();
    let head = if lines.len() > MAX_INDEX_LINES {
        lines[..MAX_INDEX_LINES].join("\n")
    } else {
        trimmed.to_owned()
    };
    if units::len(&head) <= MEMORY_MAX_CHARS {
        return head;
    }
    // `lastIndexOf("\n", 25000)`: the last newline at or before unit 25000.
    let window = units::slice(&head, 0, MEMORY_MAX_CHARS + 1);
    let cut = window.rfind('\n').map_or(0, |at| units::len(&window[..at]));
    units::slice(&head, 0, if cut > 0 { cut } else { MEMORY_MAX_CHARS })
}

/// `cwd` as Claude names a project dir in its projects root for memory:
/// every `/` and `.` becomes `-`.
pub fn encode_slug(cwd: &str) -> String {
    cwd.replace(['/', '.'], "-")
}

/// Global, project and memory context files for a session run in `cwd`.
/// `home` and `projects_root` belong to the root that owns the session.
pub fn gather_on_disk_context_files(
    cwd: Option<&str>,
    home: &str,
    projects_root: &str,
) -> Vec<OnDiskContextFile> {
    let _span = info_span!("Sessions.gatherContextFiles").entered();
    let mut files: Vec<OnDiskContextFile> = Vec::new();
    files.extend(read_context_file(
        &slash_join(&[home, "CLAUDE.md"]),
        "~/.claude/CLAUDE.md (global)",
        ContextScope::Global,
    ));
    for path in rule_paths(&slash_join(&[home, "rules"])) {
        files.extend(read_context_file(&path, &path, ContextScope::Global));
    }
    let Some(cwd) = cwd.filter(|c| !c.is_empty()) else {
        return files;
    };
    let (paths, memory_root) = project_context(cwd, home);
    for path in paths {
        files.extend(read_context_file(&path, &path, ContextScope::Project));
    }
    let mem_path = slash_join(&[
        projects_root,
        &encode_slug(&memory_root),
        "memory",
        "MEMORY.md",
    ]);
    if let Some(text) = read_context(&mem_path).and_then(|r| r.text) {
        let capped = cap_memory(&text);
        #[expect(clippy::cast_precision_loss, reason = "file sizes fit f64")]
        let bytes = text::to_display(&capped).len() as f64;
        files.push(OnDiskContextFile {
            label: "memory/MEMORY.md".to_owned(),
            path: mem_path,
            bytes,
            tokens_est: units::est_tokens(&capped),
            scope: ContextScope::Memory,
        });
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert_eq!(
            rule_globs("---\npaths: src/**, \"a/{b,c}\"\n---\nbody"),
            ["src", "a/{b,c}"]
        );
        assert_eq!(
            rule_globs("---\r\npaths:\r\n  - '**'\r\n  - x/**\r\n---"),
            ["**", "x"]
        );
        assert_eq!(rule_globs("---\npaths: [a, b]\n---"), ["a", "b"]);
        assert_eq!(rule_globs("no frontmatter"), Vec::<String>::new());
        assert!(!is_conditional_rule("---\npaths: **\n---\n"));
        assert!(is_conditional_rule("\u{FEFF}---\npaths: x\n---"));
    }

    #[test]
    fn dirs_and_slugs() {
        assert_eq!(project_dirs("/a/b/"), ["/a", "/a/b"]);
        assert_eq!(project_dirs("/"), Vec::<String>::new());
        assert_eq!(encode_slug("/Users/x/.cfg"), "-Users-x--cfg");
        assert_eq!(main_root_of("/w", "gitdir: /m/.git/worktrees/w\n"), "/m");
        assert_eq!(main_root_of("/w", "gitdir: ../x"), "/w");
    }

    #[test]
    fn memory_cap() {
        let long = (0..300)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(cap_memory(&long).lines().count(), 200);
        let wide = format!("{}\n{}", "a".repeat(20_000), "b".repeat(10_000));
        assert_eq!(cap_memory(&wide).len(), 20_000);
    }
}
