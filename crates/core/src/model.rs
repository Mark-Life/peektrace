//! Domain types shared by discovery, parsing and listing.

use std::fmt;

/// A coding agent whose sessions peektrace reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Agent {
    Claude,
    Codex,
    Pi,
    Opencode,
}

impl Agent {
    /// Every agent, in matrix order.
    pub const ALL: [Self; 4] = [Self::Claude, Self::Codex, Self::Pi, Self::Opencode];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Pi => "pi",
            Self::Opencode => "opencode",
        }
    }

    /// True when this build can read the agent's sessions.
    pub const fn has_reader(self) -> bool {
        matches!(self, Self::Claude)
    }

    /// Exact, case-sensitive match on the agent id.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.as_str() == s)
    }
}

impl fmt::Display for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

macro_rules! string_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_id!(
    /// Session id: the transcript file name without `.jsonl`.
    SessionId
);
string_id!(
    /// Claude project slug: the transcript's parent directory name.
    ProjectSlug
);

/// Which configured root a transcript came from. Set only when more than one
/// root is configured. Both fields are transcript text (see
/// [`crate::js::text::units`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTag {
    pub id: String,
    pub label: String,
}

/// File size and mtime, as `fs.stat` reports them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileStamp {
    pub size_bytes: u64,
    pub mtime_ms: i64,
}

/// One transcript located on disk, before its body is read. `stamp` is from
/// the discovery `stat`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFileRef {
    pub id: SessionId,
    pub slug: ProjectSlug,
    pub path: String,
    pub stamp: FileStamp,
    pub source: Option<SourceTag>,
}

/// The list-view summary of one session. Field order is the `--json` key order.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionHeader {
    pub id: SessionId,
    pub agent: Agent,
    pub path: String,
    pub project: ProjectSlug,
    pub message_count: u64,
    pub size_bytes: u64,
    pub updated_at: String,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub model: Option<String>,
    pub title: Option<String>,
    pub started_at: Option<String>,
    pub malformed_lines: Option<u64>,
    pub cost_usd: Option<f64>,
    pub unpriced_models: Option<Vec<String>>,
    pub source: Option<SourceTag>,
}

/// A transcript that was listed but could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadableSession {
    pub agent: Agent,
    pub project: String,
    pub path: String,
}

/// Every readable header plus the transcripts that failed to read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionListing {
    pub headers: Vec<SessionHeader>,
    pub unreadable: Vec<UnreadableSession>,
    /// Agents the filter asked for that have no reader, so nothing was listed.
    pub skipped: Vec<Agent>,
}
