//! peektrace core: find coding-agent sessions on disk, fold each transcript
//! into a list header, and select the rows `sessions ls` shows.

pub mod capabilities;
pub mod claude;
pub mod codex;
mod discover;
pub mod export;
mod headers;
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
pub mod stats;

pub use listing::{AgentRoots, ListFilter, list_sessions};
pub use model::{
    Agent, FileStamp, ProjectSlug, SessionFileRef, SessionHeader, SessionId, SessionListing,
    SourceTag, UnreadableSession,
};
pub use paths::PathStyle;
