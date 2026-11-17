//! The small marks beside a session: launcher, harness and model provider
//! monograms with their tooltips, and the worktree note.

use peektrace_protocol::AgentId;
use peektrace_protocol::marks::{UNKNOWN_PROVIDER, provider_line, worktree_hint};
pub use peektrace_protocol::marks::{harness_mono, harness_name, launcher_mono};
use peektrace_protocol::wire::sessions::{Launcher, ModelProvider, Worktree};

/// The agent of an analysis's `provider` (`claude-code`, `codex`, …).
pub fn agent_of_provider(provider: &str) -> Option<AgentId> {
    match provider {
        "claude-code" => Some(AgentId::Claude),
        other => AgentId::parse(other),
    }
}

/// The provider mark's tooltip: the main model's line, then every other
/// model, one per line. `None` when the session names no model.
pub fn provider_help(
    p: Option<&ModelProvider>,
    model: Option<&str>,
    models: &[String],
) -> Option<String> {
    let main = model
        .filter(|m| !m.is_empty())
        .or_else(|| models.first().map(String::as_str))?;
    let maker = p.map(|p| p.name.as_str());
    let lines = std::iter::once(provider_line(maker, main)).chain(
        models
            .iter()
            .filter(|m| m.as_str() != main)
            .map(String::clone),
    );
    Some(lines.collect::<Vec<_>>().join("\n"))
}

/// The provider's accessible name: its name, else "Unknown provider".
pub fn provider_name(p: Option<&ModelProvider>) -> &str {
    p.map_or(UNKNOWN_PROVIDER, |p| p.name.as_str())
}

/// The worktree marker's tooltip: the worktree's path, then who started
/// the session when known.
pub fn worktree_help(w: &Worktree, launcher: Option<&Launcher>) -> String {
    worktree_hint(&w.path, launcher.and_then(|l| l.parent_title.as_deref()))
}

#[cfg(test)]
mod tests;
