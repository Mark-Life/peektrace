//! The text the memory views show, and the inspector's wording of API
//! errors (`lib/wire-error.ts`).

use peektrace_protocol::view::memory::{DeleteView, ProjectSummaryView};
use peektrace_protocol::{ApiError, FileChangeReason};

/// CSS `text-transform: capitalize`: each word's first letter upper case.
pub fn capitalize(s: &str) -> String {
    let mut start = true;
    s.chars()
        .flat_map(|c| {
            let up = start && !c.is_whitespace();
            start = c.is_whitespace();
            if up {
                c.to_uppercase().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `13 files · indexed`.
pub fn overview_line(p: &ProjectSummaryView) -> String {
    let n = usize::try_from(p.file_count).unwrap_or(usize::MAX);
    format!(
        "{}{}",
        plural(n, "file", "files"),
        if p.has_index {
            " · indexed"
        } else {
            " · no MEMORY.md"
        }
    )
}

pub fn table_heading(shown: usize, total: usize) -> String {
    format!("{shown} of {total} memories")
}

pub fn orphan_heading(n: usize) -> String {
    format!(
        "{} (not in index)",
        plural(n, "orphan file", "orphan files")
    )
}

pub fn dangling_heading(n: usize) -> String {
    format!(
        "{} (file missing)",
        plural(n, "dangling pointer", "dangling pointers")
    )
}

/// The drawn text of the below-fold note (CSS upper-cases its tail).
pub fn below_fold_text(n: u64) -> String {
    format!(
        "{n} entr{} below the fold — INVISIBLE TO CLAUDE",
        if n == 1 { "y is" } else { "ies are" }
    )
}

pub fn empty_description(can_write: bool) -> String {
    format!(
        "No project has a memory folder with files yet. {}",
        if can_write {
            "Create the first memory in a Claude project, or let Claude save one."
        } else {
            "Memories appear here once Claude saves one."
        }
    )
}

/// The create dialog's line; `project` is the slug for a fixed target.
pub fn create_description(name: &str, project: Option<&str>) -> String {
    let name = if name.is_empty() { "<name>" } else { name };
    format!(
        "Adds {name}.md and a MEMORY.md index line in {}.",
        project.unwrap_or("<project>")
    )
}

pub fn delete_description(slug: &str, file_name: &str) -> String {
    format!(
        "Removes {file_name} and its MEMORY.md index line. Any [[{slug}]] links elsewhere will be reported as dangling."
    )
}

pub fn conflict_text(file_name: &str) -> String {
    format!(
        "{file_name} was modified after you opened it. Reload to pull the latest (your edits are discarded) or overwrite to force-save."
    )
}

/// The toast after a delete: title, description, and whether it warns.
pub fn deleted_toast(slug: &str, done: &DeleteView) -> (String, Option<String>, bool) {
    let n = done.dangling.len();
    if n == 0 {
        return (format!("Deleted {slug}"), None, false);
    }
    let items = done
        .dangling
        .iter()
        .map(|d| format!("{} → {}", d.from, d.target))
        .collect::<Vec<_>>()
        .join(", ");
    (
        format!(
            "Deleted {slug} — {} now dangling",
            plural(n, "reference", "references")
        ),
        Some(items),
        true,
    )
}

/// The text the inspector shows for a typed API error.
pub fn wire_error_message(e: &ApiError) -> String {
    match e {
        ApiError::FileChangedError { reason, .. } => {
            let r = match reason {
                FileChangeReason::Mtime => "mtime",
                FileChangeReason::Hash => "hash",
                FileChangeReason::Missing => "missing",
            };
            format!("The file changed on disk ({r}) since you loaded it.")
        }
        ApiError::MemoryValidationError { reason, .. } => reason.clone(),
        ApiError::MemoryNotFoundError { project, name } => {
            format!("Memory \"{name}\" not found in {project}.")
        }
        ApiError::CapabilityUnsupportedError {
            capability_id,
            agent_id,
        } => format!("{} does not support {capability_id}.", agent_id.as_str()),
        ApiError::PathOutsideRootError { path, .. } => {
            format!("Refused: {path} is outside the agent roots.")
        }
        ApiError::SessionNotFoundError { id, .. } => format!("Session \"{id}\" was not found."),
        ApiError::TranscriptParseError { reason, .. } => format!(
            "That transcript could not be read ({reason}). The file may be locked or malformed."
        ),
        ApiError::SettingsWriteError { reason, .. } => {
            format!("Settings could not be saved ({reason}).")
        }
        ApiError::BadRequestError { reason } => {
            format!("The server refused the request ({reason}).")
        }
        ApiError::ImageNotFoundError { .. } => "That image is not in the transcript.".to_owned(),
        ApiError::NotFoundError { path } => format!("No such API route: {path}."),
        ApiError::MethodNotAllowedError { method } => {
            format!("The server does not accept {method} here.")
        }
        ApiError::StatsScanError { .. } | ApiError::StatsRowNotFoundError { .. } => {
            "The request failed.".to_owned()
        }
    }
}
