//! `peektrace stats`: fold every transcript into fact rows, cache them per
//! file, and rank what fails, what costs time and what a rule could fix.

pub mod accumulate;
pub mod detectors;
pub mod normalize;
pub mod rank;
pub mod redact;
pub mod schema;
pub mod shell_lex;
