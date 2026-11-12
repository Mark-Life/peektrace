//! Claude memory: the read model (projects, vaults, link graph, index
//! budget) and safe writes (create, update, delete).

pub mod frontmatter;
pub mod fs;
pub mod graph;
pub mod index_edit;
pub mod json;
pub mod parse;
pub mod read;
pub mod resolve;
pub mod types;
pub mod write;

pub use read::{
    AllVaults, ProjectLabel, all_vaults, build_vault, get_vault, list_projects, memory_dir,
};
pub use types::*;
pub use write::{
    FrontmatterPatch, MemoryCtx, MemoryError, MemoryUpdate, NewMemory, create, delete, update,
};
