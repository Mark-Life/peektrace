//! JavaScript semantics the TS CLI output depends on: number and date
//! formatting, `Date.parse`, `JSON.parse` leniency, string trimming and
//! ordering. The IO-free parts live in `peektrace_js`, shared with the
//! wasm build of the app.

pub use peektrace_js::{json, json_out, num, text, units, value};

pub mod os;
pub mod regex;
pub mod system_zone;
pub mod time;
