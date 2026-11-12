//! Memory-file paths: `@imports` in a memory file, and the dirs Codex's
//! project walk reads.

use std::collections::HashMap;

use super::detectors::rules::CodexCover;
use super::memory_files::{ancestors, dirname, join};
use super::rank::cmp_str;
use crate::js::regex::LazyRe;
use crate::js_re;
use crate::sessions::resolve::exists;

static ABSOLUTE: LazyRe = js_re!(r"^(?:\/|[A-Za-z]:\/)");
static IMPORT: LazyRe = js_re!(r"(?:^|\s)@((?:~\/|\.{1,2}\/|\/)?[\w.~/-]+)", "g");
static PATHLIKE: LazyRe = js_re!(r"[/.]");
static FENCE: LazyRe = js_re!(r"^\s*(?:```|~~~)");
static CODE_SPAN: LazyRe = js_re!(r"`[^`]*`", "g");
static TRAILING_PUNCT: LazyRe = js_re!(r"[.,;:)]+$");

fn is_under(path: &str, dir: &str) -> bool {
    path == dir
        || path.starts_with(&if dir == "/" {
            "/".to_owned()
        } else {
            format!("{dir}/")
        })
}

/// For each Codex project dir, how its walk reaches the cwds under it.
pub(super) fn codex_dirs_of(
    project_dirs: &[String],
    user_home: Option<&str>,
) -> HashMap<String, CodexCover> {
    let mut read: Vec<(String, bool)> = Vec::new();
    let mut roots: Vec<String> = Vec::new();
    for cwd in project_dirs {
        let chain = ancestors(cwd, user_home);
        let mut walk = chain[..chain.len().min(1)].to_vec();
        let mut rooted = false;
        for (i, dir) in chain.iter().enumerate() {
            if exists(&join(&[dir, ".git"])) {
                walk = chain[..=i].to_vec();
                rooted = true;
                if !roots.contains(dir) {
                    roots.push(dir.clone());
                }
                break;
            }
        }
        for dir in walk {
            match read.iter_mut().find(|(d, _)| *d == dir) {
                Some(entry) => entry.1 = entry.1 || rooted,
                None => read.push((dir, rooted)),
            }
        }
    }
    read.into_iter()
        .map(|(dir, rooted)| {
            let mut stops: Vec<String> = if rooted {
                roots
                    .iter()
                    .filter(|r| **r != dir && is_under(r, &dir))
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };
            stops.sort_by(|a, b| cmp_str(a, b));
            (
                dir,
                CodexCover {
                    exact: !rooted,
                    stops,
                },
            )
        })
        .collect()
}

fn resolve_import(raw: &str, from: &str, user_home: Option<&str>) -> Option<String> {
    if let Some(rest) = raw.strip_prefix("~/") {
        return user_home.map(|h| join(&[h, rest]));
    }
    Some(if ABSOLUTE.is_match(raw) {
        raw.to_owned()
    } else {
        join(&[dirname(from), raw])
    })
}

fn prose_of(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in lines {
        if FENCE.is_match(line) {
            fenced = !fenced;
        } else if !fenced {
            out.push(CODE_SPAN.replace_all_str(line, ""));
        }
    }
    out
}

pub(super) fn imports_of(lines: &[String], from: &str, user_home: Option<&str>) -> Vec<String> {
    prose_of(lines)
        .iter()
        .flat_map(|line| {
            IMPORT
                .captures_all(line)
                .iter()
                .filter_map(|m| {
                    let raw =
                        TRAILING_PUNCT.replace_first(m.get(1).unwrap_or(""), |_| String::new());
                    if PATHLIKE.is_match(&raw) {
                        resolve_import(&raw, from, user_home)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}
