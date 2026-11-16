//! Named causes of failed calls: the catalog and the per-call detectors.

mod catalog;
mod detect;

pub use catalog::{
    CauseKind, CauseSpec, FILE_EDIT_TOOLS, GIT_DIFF_FIX, PATH_FILES, Per, SHELL_TOOLS, WRITE_TOOLS,
    git_diff_probes, is_shell_tool, probes, spec,
};
pub use detect::{CauseHit, Scrub, bin_use, detect_cause, escape_re, glob_value, harness_body};
