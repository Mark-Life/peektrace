//! Exact shapes of the JSON the core writers print for sessions, memory and
//! stats. The writers do not use these types; `crates/server/tests/wire.rs`
//! reads real answers into them and back, so the two cannot drift. The
//! inspector's TS types are generated from them.

pub mod events;
pub mod memory;
pub mod sessions;
pub mod stats;
