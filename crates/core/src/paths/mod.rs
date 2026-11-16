//! Path strings with Node `path` semantics for the platform's style. Paths
//! stay strings because they are output verbatim and compared as strings.

pub(crate) mod posix;
mod win32;

/// Which `node:path` flavour to follow.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PathStyle {
    #[default]
    Posix,
    Windows,
}

impl PathStyle {
    /// The style of the platform this binary runs on.
    pub const NATIVE: Self = if cfg!(windows) {
        Self::Windows
    } else {
        Self::Posix
    };

    /// `path.delimiter`: the separator of `PATH`-style lists.
    pub const fn delimiter(self) -> char {
        match self {
            Self::Posix => ':',
            Self::Windows => ';',
        }
    }

    /// `path.join`.
    pub fn join(self, parts: &[&str]) -> String {
        match self {
            Self::Posix => posix::join(parts),
            Self::Windows => win32::join(parts),
        }
    }

    /// `path.resolve(path)` with `cwd` as the working directory.
    pub fn resolve(self, cwd: &str, path: &str) -> String {
        match self {
            Self::Posix => posix::resolve(cwd, path),
            Self::Windows => win32::resolve(cwd, path),
        }
    }

    /// `path.basename`.
    pub fn basename(self, path: &str) -> &str {
        match self {
            Self::Posix => posix::basename(path),
            Self::Windows => win32::basename(path),
        }
    }

    /// True when `resolve(cwd, path)` does not depend on `cwd`.
    pub fn ignores_cwd(self, path: &str) -> bool {
        match self {
            Self::Posix => path.starts_with('/'),
            Self::Windows => win32::is_fully_qualified(path),
        }
    }
}

/// How Claude Code names a project dir: every UTF-16 unit outside
/// `[a-zA-Z0-9]` becomes `-`.
pub fn claude_slug(path: &str) -> String {
    path.chars()
        .flat_map(|c| {
            let n = if c.is_ascii_alphanumeric() {
                0
            } else {
                c.len_utf16()
            };
            std::iter::repeat_n('-', n).chain(c.is_ascii_alphanumeric().then_some(c))
        })
        .collect()
}

/// A Windows verbatim path as Node `fs.realpath` prints it: `\\?\C:\x` is
/// `C:\x` and `\\?\UNC\host\share` is `\\host\share`. Others pass through.
pub fn strip_verbatim(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    match path.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_owned(),
        _ => path.to_owned(),
    }
}

/// `fs.realpath(path)`, or `path` itself when it cannot be resolved.
pub fn real_path(path: &str) -> String {
    std::fs::canonicalize(path).map_or_else(
        |_| path.to_owned(),
        |p| strip_verbatim(&p.to_string_lossy()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_windows_verbatim_prefixes() {
        for (raw, want) in [
            (r"\\?\C:\Users\x\CLAUDE.md", r"C:\Users\x\CLAUDE.md"),
            (r"\\?\UNC\host\share\a.md", r"\\host\share\a.md"),
            (r"\\?\Volume{1}\a.md", r"\\?\Volume{1}\a.md"),
            ("/home/x/CLAUDE.md", "/home/x/CLAUDE.md"),
        ] {
            assert_eq!(strip_verbatim(raw), want);
        }
    }

    #[test]
    fn real_path_has_no_verbatim_prefix() {
        let dir = std::env::temp_dir();
        assert!(!real_path(&dir.to_string_lossy()).starts_with(r"\\?\"));
    }
}
