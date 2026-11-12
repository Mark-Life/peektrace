//! `peektrace stats`: fold every transcript into fact rows, cache them per
//! file, and rank what fails, what costs time and what a rule could fix.
#![expect(
    clippy::float_cmp,
    reason = "numbers follow JS, which compares them exactly"
)]

pub mod accumulate;
pub mod cache;
pub mod detectors;
pub mod extract;
pub mod findings;
pub mod format;
pub mod host_bins;
pub mod icu_zones;
pub mod markers;
pub mod memory_files;
mod memory_paths;
pub mod normalize;
pub mod pass;
pub mod prompts;
pub mod rank;
pub mod redact;
pub mod report;
pub mod rule_dates;
pub mod schema;
pub mod service;
pub mod shell_ctx;
pub mod shell_lex;
pub mod usage_service;
pub mod walk;
