//! The detectors: per-call classifiers and the tables they fold into.

mod cause_agg;
pub mod cause_row;
pub mod causes;
pub mod cells;
pub mod clustering;
pub mod corpus;
pub mod corpus_rows;
pub mod failures;
pub mod pairing;
pub mod retries;
pub mod retry_diff;
pub mod rules;
pub mod similarity;
pub mod timing;
pub mod usage;
pub mod waiting;
