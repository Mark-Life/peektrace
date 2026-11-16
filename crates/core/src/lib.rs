//! peektrace core: find coding-agent sessions on disk, fold each transcript
//! into a list header, and select the rows `sessions ls` shows.

pub mod claude;
pub mod export;
pub mod js;
pub mod listing;
pub mod model;
mod paths;
mod pricing;
pub mod roots;
pub mod select;
pub mod sessions;

pub use listing::{AgentRoots, ListFilter, list_sessions};
pub use model::{
    Agent, FileStamp, ProjectSlug, SessionFileRef, SessionHeader, SessionId, SessionListing,
    SourceTag, UnreadableSession,
};
pub use paths::PathStyle;
