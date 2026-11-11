//! peektrace core: find coding-agent sessions on disk, fold each transcript
//! into a list header, and select the rows `sessions ls` shows.

mod claude;
pub mod js;
pub mod listing;
pub mod model;
mod paths;
mod pricing;
pub mod roots;
pub mod select;

pub use listing::{AgentRoots, ListFilter, list_sessions};
pub use model::{
    Agent, ProjectSlug, SessionFileRef, SessionHeader, SessionId, SessionListing, SourceTag,
    UnreadableSession,
};
pub use paths::PathStyle;
