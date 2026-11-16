//! Where the views get their data. Native builds call core on worker
//! threads ([`in_process`]); the web build fetches the local server's
//! `/api` and listens on `/api/events` ([`http`]). Both hand the views the
//! same typed shapes, decoded from the same JSON.

#[cfg(not(target_arch = "wasm32"))]
pub mod in_process;

#[cfg(target_arch = "wasm32")]
pub mod http;

use peektrace_protocol::view::{AnalyzedSession, SessionListingView, mend_surrogates};
use peektrace_protocol::{ApiError, ApiErrorBody, WatchVersions};

/// Why a load failed.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadError {
    /// The server or core answered with a typed error.
    Api(ApiError),
    Transport {
        status: Option<u16>,
        message: String,
    },
    /// The body did not match the view types.
    Decode(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Api(ApiError::SessionNotFoundError { id, .. }) => {
                write!(f, "Session {id} was not found.")
            }
            Self::Api(ApiError::TranscriptParseError { path, reason }) => {
                write!(f, "Could not read {path}: {reason}")
            }
            Self::Api(e) => write!(f, "{e:?}"),
            Self::Transport {
                status: Some(s),
                message,
            } => write!(f, "HTTP {s}: {message}"),
            Self::Transport {
                status: None,
                message,
            } => f.write_str(message),
            Self::Decode(m) => write!(f, "Unexpected response: {m}"),
        }
    }
}

pub type Reply<T> = Box<dyn FnOnce(Result<T, LoadError>) + Send + 'static>;
/// What a source's watch reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    Versions(WatchVersions),
    /// Changes that will not reach the app, one sentence each.
    Problems(Vec<String>),
    /// The live link came back after a failure. A restarted server counts
    /// from 0 again, so no bump would show what changed meanwhile.
    Reconnected,
}

pub type OnVersions = Box<dyn Fn(WatchEvent) + Send + Sync + 'static>;

pub trait DataSource: Send + Sync + 'static {
    fn sessions(&self, reply: Reply<SessionListingView>);
    fn analysis(&self, id: String, parent: Option<String>, reply: Reply<AnalyzedSession>);
    /// Calls `on_versions` with the current versions, then after each change
    /// and on each reconnect.
    fn watch(&self, on_versions: OnVersions);
    /// The listing now, for headless runs; `None` where loads cannot block.
    fn sessions_now(&self) -> Option<Result<SessionListingView, LoadError>> {
        None
    }
    fn analysis_now(
        &self,
        _id: &str,
        _parent: Option<&str>,
    ) -> Option<Result<AnalyzedSession, LoadError>> {
        None
    }
}

/// Decode a JSON body into a view type, reading lone surrogates as U+FFFD.
pub fn decode<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, LoadError> {
    serde_json::from_str(&mend_surrogates(body)).map_err(|e| LoadError::Decode(e.to_string()))
}

/// Decode an error body (`{"error": {...}}`), else a transport error.
pub fn decode_error(status: u16, body: &str) -> LoadError {
    serde_json::from_str::<ApiErrorBody>(body).map_or_else(
        |_| LoadError::Transport {
            status: Some(status),
            message: body.chars().take(200).collect(),
        },
        |b| LoadError::Api(b.error),
    )
}
