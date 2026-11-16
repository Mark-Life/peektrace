//! Settings form rules: editable rows from the stored document and back,
//! the default root per agent, home paths as `~`, wire error text and the
//! page copy.

use peektrace_js::text::trim;
use peektrace_protocol::{
    AgentId, ApiError, FileChangeReason, PeektraceSettings, ScannedRoot, ServerInfo,
};
use serde_json::{Map, Value};

/// Card order: `AGENT_IDS`.
pub const AGENT_ORDER: [AgentId; 4] = AgentId::ALL;

/// Editable `(path, label)` rows per agent, in [`AGENT_ORDER`].
pub type Rows = [Vec<(String, String)>; 4];

/// Rows seeded from loaded settings; entries that are not
/// `{path: string, label?: string}` are skipped.
pub fn seed_rows(s: &PeektraceSettings) -> Rows {
    AGENT_ORDER.map(|a| {
        s.roots
            .as_ref()
            .and_then(|r| r.get(a.as_str()))
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|e| {
                        let path = e.get("path")?.as_str()?;
                        let label = e.get("label").and_then(Value::as_str).unwrap_or("");
                        Some((path.to_owned(), label.to_owned()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// The rows back as settings: JS-trimmed, empty paths dropped, empty labels
/// left out, agents with no rows omitted; `{}` when none are left.
pub fn assemble<P: AsRef<str>, L: AsRef<str>>(rows: &[Vec<(P, L)>; 4]) -> PeektraceSettings {
    let roots: Map<String, Value> = AGENT_ORDER
        .iter()
        .zip(rows)
        .filter_map(|(agent, list)| {
            let cleaned: Vec<Value> = list
                .iter()
                .map(|(p, l)| (trim(p.as_ref()), trim(l.as_ref())))
                .filter(|(p, _)| !p.is_empty())
                .map(|(p, l)| {
                    let mut entry = Map::new();
                    entry.insert("path".into(), Value::from(p));
                    if !l.is_empty() {
                        entry.insert("label".into(), Value::from(l));
                    }
                    Value::Object(entry)
                })
                .collect();
            (!cleaned.is_empty()).then(|| (agent.as_str().to_owned(), Value::Array(cleaned)))
        })
        .collect();
    PeektraceSettings {
        roots: (!roots.is_empty()).then_some(roots),
    }
}

/// The scanned default folder per listed agent, as a JS `Map` built from
/// entries: a repeated agent keeps its first place and its last value.
pub fn default_roots(info: &ServerInfo) -> Vec<(AgentId, &ScannedRoot)> {
    let mut out: Vec<(AgentId, &ScannedRoot)> = Vec::new();
    for scan in &info.agents {
        let Some(root) = scan.roots.iter().find(|r| r.is_default) else {
            continue;
        };
        match out.iter_mut().find(|(a, _)| *a == scan.agent) {
            Some(slot) => slot.1 = root,
            None => out.push((scan.agent, root)),
        }
    }
    out
}

/// `path` with the home dir shown as `~`; `/` and `\` both separate.
pub fn tilde_path(path: &str, home: &str) -> String {
    let base = home.trim_end_matches(['/', '\\']);
    if base.is_empty() {
        return path.to_owned();
    }
    if path == base {
        return "~".to_owned();
    }
    match path.strip_prefix(base) {
        Some(rest) if rest.starts_with(['/', '\\']) => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

const fn change_reason(r: FileChangeReason) -> &'static str {
    match r {
        FileChangeReason::Mtime => "mtime",
        FileChangeReason::Hash => "hash",
        FileChangeReason::Missing => "missing",
    }
}

/// A sentence for a typed API error (`wireErrorMessage`).
pub fn wire_error_message(e: &ApiError) -> String {
    match e {
        ApiError::FileChangedError { reason, .. } => format!(
            "The file changed on disk ({}) since you loaded it.",
            change_reason(*reason)
        ),
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

/// A run of copy, drawn plain or as inline code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    Plain(String),
    Code(String),
}

/// The words of `segs`, as a reader sees them.
pub fn seg_text(segs: &[Seg]) -> String {
    segs.iter()
        .map(|s| match s {
            Seg::Plain(t) | Seg::Code(t) => t.as_str(),
        })
        .collect()
}

fn plain(s: &str) -> Seg {
    Seg::Plain(s.to_owned())
}

fn code(s: &str) -> Seg {
    Seg::Code(s.to_owned())
}

/// The native app reads its roots at launch; the web build talks to a server.
pub const NATIVE: bool = cfg!(not(target_arch = "wasm32"));

pub mod copy {
    pub const HEADING: &str = "Settings";
    pub const HEADING_NOTE: &str = "Point peektrace at extra agent config dirs — e.g. a separate work account — to scan them in parallel.";
    pub const RESTART_TITLE: &str = "Changes apply after a restart";
    pub const READ_ONLY_TITLE: &str = "Read-only server";
    pub const CONFLICT_TITLE: &str = "settings.json changed on disk";
    pub const CONFLICT_BODY: &str = "The file was modified after you loaded it. Reload to pull the latest (your edits are discarded) or overwrite to force-save what you have.";
    pub const CONFLICT_RELOAD: &str = "Reload from disk";
    pub const CONFLICT_OVERWRITE: &str = "Overwrite";
    pub const AGENT_NOTE: &str = "Extra config dirs to scan alongside the default. The transcript root is derived from each path per agent layout.";
    pub const DEFAULT_LABEL: &str = "Default";
    pub const NOT_FOUND: &str = "not found";
    pub const NO_ROOTS: &str = "No extra roots.";
    pub const PATH_LABEL: &str = "Path";
    pub const PATH_PLACEHOLDER: &str = "~/work/.claude";
    pub const LABEL_LABEL: &str = "Label";
    pub const LABEL_PLACEHOLDER: &str = "work";
    pub const REMOVE_ROOT: &str = "Remove root";
    pub const ADD_ROOT: &str = "Add root";
    pub const SAVE: &str = "Save settings";
    pub const SAVING: &str = "Saving…";
    pub const CAPS_TITLE: &str = "Capabilities";
    pub const CAPS_NOTE: &str = "What each agent supports";
    pub const CAPS_OPEN: &str = "Open";
    pub const SAVED: &str = "Settings saved";
    pub const SAVE_FAILED: &str = "Save failed.";
    pub const TYPED_FAILURE: &str = "Something went wrong";
    pub const UNTYPED_FAILURE: &str = "Request failed";
    pub const RETRY: &str = "Retry";
    pub const RETRYING: &str = "Retrying…";
}

/// The restart notice under its title, with the settings file shown as `shown`.
pub fn restart_body(shown: &str, native: bool) -> Vec<Seg> {
    if native {
        vec![
            plain("Saved to "),
            code(shown),
            plain(
                ". Peektrace scans the roots it read at launch, so restart Peektrace for new roots to appear in the session list.",
            ),
        ]
    } else {
        vec![
            plain("Saved to "),
            code(shown),
            plain(". The running server scans the roots it read at startup, so restart "),
            code("peektrace serve"),
            plain(" for new roots to appear in the session list."),
        ]
    }
}

/// The toast note after a save.
pub const fn saved_note(native: bool) -> &'static str {
    if native {
        "Restart Peektrace to apply to the session list."
    } else {
        "Restart `peektrace serve` to apply to the session list."
    }
}

/// Why a read-only server refuses writes, and what that means here.
pub fn read_only_body() -> Vec<Seg> {
    vec![
        plain("The server was started with "),
        code("--read-only"),
        plain(", or on a non-loopback "),
        code("--host"),
        plain(" without "),
        code("--allow-write"),
        plain(". Settings can't be saved from here."),
    ]
}

/// What a default root scans.
pub fn scans(shown: &str) -> Vec<Seg> {
    vec![plain("scans "), code(shown)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use peektrace_protocol::AgentScan;
    use serde_json::json;

    const HOME: &str = "/home/dev";

    fn settings(v: Value) -> PeektraceSettings {
        serde_json::from_value(v).unwrap_or_default()
    }

    #[test]
    fn tilde_collapses_home() {
        assert_eq!(tilde_path(HOME, HOME), "~");
        assert_eq!(
            tilde_path("/home/dev/.claude/projects", HOME),
            "~/.claude/projects"
        );
        assert_eq!(tilde_path("/home/devx/.claude", HOME), "/home/devx/.claude");
        assert_eq!(tilde_path("/var/data/sessions", HOME), "/var/data/sessions");
        assert_eq!(tilde_path("/home/dev/.pi", "/home/dev/"), "~/.pi");
        assert_eq!(tilde_path("/a/b", ""), "/a/b");
        let home = "C:\\Users\\dev";
        assert_eq!(
            tilde_path("C:\\Users\\dev\\.claude\\projects", home),
            "~\\.claude\\projects"
        );
        assert_eq!(
            tilde_path("C:\\Users\\dev\\.pi", "C:\\Users\\dev\\"),
            "~\\.pi"
        );
        assert_eq!(tilde_path("C:\\Users\\devx", home), "C:\\Users\\devx");
    }

    #[test]
    fn seed_and_assemble_round_trip() {
        let s = settings(json!({"roots": {
            "codex": [{"path": "~/c", "label": "work"}],
            "claude": [{"path": "~/a"}, {"nope": 1}],
            "bogus": [{"path": "~/x"}]
        }}));
        let rows = seed_rows(&s);
        assert_eq!(rows[0], [("~/a".to_owned(), String::new())]);
        assert_eq!(rows[1], [("~/c".to_owned(), "work".to_owned())]);
        assert!(rows[2].is_empty() && rows[3].is_empty());
        let back = serde_json::to_value(assemble(&rows)).unwrap_or_default();
        assert_eq!(
            back,
            json!({"roots": {"claude": [{"path": "~/a"}], "codex": [{"path": "~/c", "label": "work"}]}})
        );
    }

    #[test]
    fn assemble_trims_like_js_and_drops_empties() {
        let rows: [Vec<(&str, &str)>; 4] = [
            vec![("  ~/a  ", " "), ("", "x"), (" \u{a0}", "y")],
            vec![("\u{feff}~/b\u{2028}", " lab ")],
            vec![("\u{85}~/c", "")],
            vec![],
        ];
        let v = serde_json::to_value(assemble(&rows)).unwrap_or_default();
        assert_eq!(
            v,
            json!({"roots": {
                "claude": [{"path": "~/a"}],
                "codex": [{"path": "~/b", "label": "lab"}],
                "pi": [{"path": "\u{85}~/c"}]
            }})
        );
        let empty: [Vec<(&str, &str)>; 4] = [vec![(" ", "")], vec![], vec![], vec![]];
        assert_eq!(assemble(&empty), PeektraceSettings::default());
    }

    #[test]
    fn defaults_pick_the_default_root() {
        let root = |d: bool, p: &str| ScannedRoot {
            is_default: d,
            home: p.into(),
            path: format!("{p}/projects"),
            exists: true,
        };
        let info = ServerInfo {
            read_only: false,
            user_home: HOME.into(),
            agents: vec![
                AgentScan {
                    agent: AgentId::Claude,
                    roots: vec![root(false, "/x"), root(true, "/a")],
                },
                AgentScan {
                    agent: AgentId::Pi,
                    roots: vec![root(false, "/p")],
                },
            ],
        };
        let d = default_roots(&info);
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].0, d[0].1.home.as_str()), (AgentId::Claude, "/a"));
    }

    #[test]
    fn wire_messages() {
        let e = ApiError::FileChangedError {
            path: "p".into(),
            reason: FileChangeReason::Missing,
        };
        assert_eq!(
            wire_error_message(&e),
            "The file changed on disk (missing) since you loaded it."
        );
        let e = ApiError::SettingsWriteError {
            path: "settings.json".into(),
            reason: peektrace_protocol::READ_ONLY_REASON.into(),
        };
        assert_eq!(
            wire_error_message(&e),
            "Settings could not be saved (read-only mode is enabled; refusing to write)."
        );
    }
}
