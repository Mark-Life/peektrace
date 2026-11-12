//! File reads and safe writes for memory files: text as `TextDecoder`
//! reads it, whole-ms mtimes, and atomic writes (temp file, then rename)
//! confined to the agent roots, with an optional mtime compare-and-swap.

use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::BuildHasher as _;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::info_span;

use crate::js::time::file_time_ms;
use crate::paths::PathStyle;
use crate::sessions::jsonl::decode;

/// Why a write found the file changed since the caller read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeReason {
    Mtime,
    Missing,
}

impl ChangeReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mtime => "mtime",
            Self::Missing => "missing",
        }
    }
}

/// A failed write.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("File changed on disk ({}): {path}. Reload and retry.", reason.as_str())]
    Changed { path: String, reason: ChangeReason },
    #[error("Refused: path escapes the allowed roots: {path}")]
    OutsideRoots { path: String },
    #[error("{path}: {source}")]
    Io { path: String, source: io::Error },
}

impl WriteError {
    /// Wrap an IO failure on `path`.
    pub fn io(path: &str) -> impl FnOnce(io::Error) -> Self {
        let path = path.to_owned();
        move |source| Self::Io { path, source }
    }
}

/// A file read as text: a leading BOM dropped, bad UTF-8 replaced.
pub fn read_text(path: &str) -> io::Result<String> {
    let _span = info_span!("Fs.readText", path).entered();
    fs::read(path).map(decode)
}

/// The file's mtime in ms as `stat().mtime.getTime()` (0 when the platform has none).
pub fn stat_mtime(path: &str) -> io::Result<i64> {
    let _span = info_span!("Fs.stat", path).entered();
    let meta = fs::metadata(path)?;
    Ok(meta.modified().map_or(0, file_time_ms))
}

/// `fs.exists`: the path (after symlinks) can be stat'ed.
pub fn exists(path: &str) -> bool {
    fs::metadata(path).is_ok()
}

/// The working directory, for resolving relative paths.
fn cwd() -> String {
    std::env::current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// True when `path` resolves (lexically) inside one of `roots`.
pub fn is_within(path: &str, roots: &[String]) -> bool {
    let style = PathStyle::NATIVE;
    let here = cwd();
    let resolved = style.resolve(&here, path);
    let sep = if style == PathStyle::Windows {
        '\\'
    } else {
        '/'
    };
    roots.iter().any(|root| {
        let base = style.resolve(&here, root);
        resolved == base
            || resolved
                .strip_prefix(&base)
                .is_some_and(|rest| rest.starts_with(sep))
    })
}

/// 16 hex chars that differ per call.
fn temp_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{:016x}", RandomState::new().hash_one(nanos))
}

/// Write `content` to `path` through a temp file and a rename. The path
/// must sit inside `allowed`. With `expected_mtime`, the file must still
/// exist with that mtime; the check runs just before the write.
pub fn atomic_write(
    path: &str,
    content: &str,
    expected_mtime: Option<i64>,
    allowed: &[String],
) -> Result<(), WriteError> {
    let _span = info_span!("Fs.atomicWrite", path).entered();
    if !is_within(path, allowed) {
        return Err(WriteError::OutsideRoots {
            path: path.to_owned(),
        });
    }
    if let Some(want) = expected_mtime {
        let changed = |reason| WriteError::Changed {
            path: path.to_owned(),
            reason,
        };
        if !exists(path) {
            return Err(changed(ChangeReason::Missing));
        }
        if stat_mtime(path).map_err(WriteError::io(path))? != want {
            return Err(changed(ChangeReason::Mtime));
        }
    }
    let style = PathStyle::NATIVE;
    let dir = std::path::Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = style.basename(path);
    let temp = style.join(&[&dir, &format!(".{name}.tmp-{}", temp_token())]);
    fs::write(&temp, content).map_err(WriteError::io(path))?;
    fs::rename(&temp, path)
        .inspect_err(|_| {
            let _ = fs::remove_file(&temp);
        })
        .map_err(WriteError::io(path))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containment() {
        let roots = vec!["/a/b".to_owned(), "/tmp/".to_owned()];
        assert!(is_within("/a/b", &roots));
        assert!(is_within("/a/b/c/../d", &roots));
        assert!(!is_within("/a/bc", &roots));
        assert!(!is_within("/a/b/../../etc", &roots));
        assert!(is_within("/tmp/x", &roots));
    }

    #[test]
    fn tokens_are_hex() {
        let t = temp_token();
        assert_eq!(t.len(), 16);
        assert!(t.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}
