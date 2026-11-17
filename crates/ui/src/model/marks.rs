//! The small marks beside a session: launcher, harness and model provider
//! monograms with their tooltips, and the worktree note.

use peektrace_protocol::AgentId;
use peektrace_protocol::wire::sessions::{Launcher, ModelProvider, Worktree};

/// What the provider mark says when the model's maker is not known.
pub const UNKNOWN_PROVIDER: &str = "Unknown provider";

/// The harness's full name.
pub const fn harness_name(a: AgentId) -> &'static str {
    match a {
        AgentId::Claude => "Claude Code",
        AgentId::Codex => "Codex",
        AgentId::Pi => "Pi",
        AgentId::Opencode => "OpenCode",
    }
}

/// The harness's monogram.
pub const fn harness_mono(a: AgentId) -> &'static str {
    match a {
        AgentId::Claude => "CC",
        AgentId::Codex => "Cx",
        AgentId::Pi => "Pi",
        AgentId::Opencode => "OC",
    }
}

/// The agent of an analysis's `provider` (`claude-code`, `codex`, …).
pub fn agent_of_provider(provider: &str) -> Option<AgentId> {
    match provider {
        "claude-code" => Some(AgentId::Claude),
        other => AgentId::parse(other),
    }
}

/// The launcher's monogram: `T3` for t3code, else its first two letters.
pub fn launcher_mono(name: &str) -> String {
    if name.eq_ignore_ascii_case("t3code") {
        return "T3".to_owned();
    }
    let mut chars = name.chars().filter(|c| c.is_alphanumeric());
    chars.next().map_or_else(
        || "?".to_owned(),
        |c| c.to_uppercase().chain(chars.next()).collect(),
    )
}

/// `Anthropic · claude-opus-4`, or `Unknown provider · <model>`.
pub fn provider_line(p: Option<&ModelProvider>, model: &str) -> String {
    let name = p.map_or(UNKNOWN_PROVIDER, |p| p.name.as_str());
    format!("{name} · {model}")
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
    let lines = std::iter::once(provider_line(p, main)).chain(
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
    let parent = launcher
        .and_then(|l| l.parent_title.as_deref())
        .filter(|t| !t.is_empty())
        .map(|t| format!("started by: {t}"));
    std::iter::once(format!("Worktree {}", w.path))
        .chain(parent)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
