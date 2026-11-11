//! `peektrace stats`: fold every transcript into fact rows, cache them per
//! file, and rank what fails, what costs time and what a rule could fix.

pub mod accumulate;
pub mod cache;
pub mod detectors;
pub mod extract;
pub mod host_bins;
pub mod memory_files;
pub mod normalize;
pub mod rank;
pub mod redact;
pub mod rule_dates;
pub mod schema;
pub mod shell_lex;
pub mod walk;
