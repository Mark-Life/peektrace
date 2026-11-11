//! Path strings with Node `path` semantics for the platform's style. Paths
//! stay strings because they are output verbatim and compared as strings.

mod posix;
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
