//! Names, monograms and tooltip lines of the small session marks: the app
//! that started a session, the harness that ran it, the maker of its model
//! and its worktree. Every UI draws these strings; the web copy is checked
//! against `tests/fixtures/session-marks.json`, as this module is.

use crate::AgentId;

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

/// The launcher's monogram: `T3` for t3code, else its first two letters or
/// digits with the first upper case; `?` when it has none.
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
pub fn provider_line(maker: Option<&str>, model: &str) -> String {
    format!("{} · {model}", maker.unwrap_or(UNKNOWN_PROVIDER))
}

/// The worktree marker's tooltip: the worktree's path, then the thread that
/// started the session when known.
pub fn worktree_hint(path: &str, parent_title: Option<&str>) -> String {
    let parent = parent_title
        .filter(|t| !t.is_empty())
        .map(|t| format!("Started by: {t}"));
    std::iter::once(format!("Worktree: {path}"))
        .chain(parent)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn fixture() -> Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/session-marks.json"
        );
        let text = std::fs::read_to_string(path).expect("read fixture");
        serde_json::from_str(&text).expect("parse fixture")
    }

    fn s(v: &Value) -> &str {
        v.as_str().unwrap_or_default()
    }

    #[test]
    fn matches_the_shared_fixture() {
        let f = fixture();
        let rows = |k: &str| f[k].as_array().cloned().unwrap_or_default();
        let harnesses = rows("harnesses");
        assert_eq!(harnesses.len(), AgentId::ALL.len());
        for h in harnesses {
            let a = AgentId::parse(s(&h["agent"])).expect("agent id");
            assert_eq!(harness_mono(a), s(&h["mono"]));
            assert_eq!(harness_name(a), s(&h["name"]));
        }
        for l in rows("launchers") {
            assert_eq!(launcher_mono(s(&l["name"])), s(&l["mono"]));
        }
        for p in rows("providerLines") {
            assert_eq!(
                provider_line(p["maker"].as_str(), s(&p["model"])),
                s(&p["line"])
            );
        }
        for w in rows("worktreeHints") {
            assert_eq!(
                worktree_hint(s(&w["path"]), w["parentTitle"].as_str()),
                s(&w["hint"])
            );
        }
    }
}
