//! peektrace core: find coding-agent sessions on disk, fold each transcript
//! into a list header, and select the rows `sessions ls` shows.

pub mod capabilities;
pub mod claude;
pub mod codex;
mod discover;
pub mod export;
pub mod header_cache;
mod headers;
pub mod highlight;
pub mod js;
pub mod listing;
pub mod memory;
pub mod model;
pub mod opencode;
mod paths;
pub mod pi;
pub mod pricing;
pub mod roots;
pub mod search;
pub mod select;
pub mod sessions;
pub mod settings_doc;
pub mod stats;
pub mod watch;

pub use listing::{AgentRoots, ListFilter, list_sessions, list_sessions_cached};
pub use model::{
    Agent, FileStamp, ProjectSlug, SessionFileRef, SessionHeader, SessionId, SessionListing,
    SourceTag, UnreadableSession,
};
pub use paths::PathStyle;
