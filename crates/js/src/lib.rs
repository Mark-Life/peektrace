//! JavaScript semantics peektrace output depends on, with no IO, so the
//! CLI, the TUI and the wasm app share one copy: number printing and
//! rounding, `Date.parse` and `toISOString`, `JSON.parse` leniency and key
//! order, and UTF-16 lengths and cuts.

pub mod json;
pub mod json_out;
pub mod num;
pub mod text;
pub mod time;
pub mod units;
pub mod value;
