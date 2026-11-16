//! Core values as the `/api` wire types. The HTTP server and the in-process
//! app source both answer through these, so the two cannot drift.

use peektrace_protocol::{
    AgentId, AgentScan, ApiError, Capability, CapabilitySupport, FileChangeReason, PerAgentSupport,
    READ_ONLY_REASON, ScannedRoot, ServerInfo, SupportLevel,
};

use crate::capabilities::{self, SupportLevel as CoreLevel};
use crate::js::json_out::Writer;
use crate::js::value::JsValue;
use crate::sessions::resolve;
use crate::settings_doc::{
    self, ChangeReason, SettingsDoc, SettingsUpdateError, StoredSettings, write_settings_doc,
};
use crate::{Agent, AgentRoots};

pub const fn agent_id(a: Agent) -> AgentId {
    match a {
        Agent::Claude => AgentId::Claude,
        Agent::Codex => AgentId::Codex,
        Agent::Pi => AgentId::Pi,
        Agent::Opencode => AgentId::Opencode,
    }
}

const fn level(l: CoreLevel) -> SupportLevel {
    match l {
        CoreLevel::Supported => SupportLevel::Supported,
        CoreLevel::Partial => SupportLevel::Partial,
        CoreLevel::Planned => SupportLevel::Planned,
        CoreLevel::Unsupported => SupportLevel::Unsupported,
    }
}

/// `GET /api/capabilities`.
pub fn capability_rows() -> Vec<Capability> {
    capabilities::list_capabilities()
        .iter()
        .map(|c| {
            let [claude, codex, pi, opencode] = c.per_agent.map(|s| CapabilitySupport {
                level: level(s.level),
                note: s.note.map(str::to_owned),
            });
            Capability {
                id: c.id.to_owned(),
                group: c.group.to_owned(),
                title: c.title.to_owned(),
                description: c.description.to_owned(),
                per_agent: PerAgentSupport {
                    claude,
                    codex,
                    pi,
                    opencode,
                },
            }
        })
        .collect()
}

/// `GET /api/server`: the scanned roots, the first of each agent the default.
pub fn server_info(roots: &AgentRoots, read_only: bool, user_home: &str) -> ServerInfo {
    ServerInfo {
        read_only,
        user_home: user_home.to_owned(),
        agents: Agent::ALL
            .into_iter()
            .map(|agent| AgentScan {
                agent: agent_id(agent),
                roots: roots
                    .of(agent)
                    .iter()
                    .enumerate()
                    .map(|(i, s)| ScannedRoot {
                        is_default: i == 0,
                        home: s.home.clone(),
                        path: s.projects_root.clone(),
                        exists: resolve::exists(&s.projects_root),
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// The `GET` and `PUT /api/settings` body.
pub fn settings_body(s: &StoredSettings) -> String {
    let mut w = Writer::new(false);
    w.begin_obj();
    write_settings_doc(w.key("settings"), &s.doc);
    w.key("mtimeMs").int(s.mtime_ms);
    w.key("path").str(&s.path);
    w.end_obj();
    w.finish()
}

/// `Err` when a read-only server refuses a settings write.
pub fn check_settings_writable(read_only: bool) -> Result<(), ApiError> {
    if read_only {
        return Err(ApiError::SettingsWriteError {
            path: "settings.json".to_owned(),
            reason: READ_ONLY_REASON.to_owned(),
        });
    }
    Ok(())
}

/// The `settings` field of a `PUT /api/settings` body, checked.
pub fn validated_settings(v: &JsValue) -> Result<SettingsDoc, ApiError> {
    settings_doc::validate_settings(v).map_err(|reason| ApiError::BadRequestError { reason })
}

/// Store `doc` after the compare-and-swap on `expected_mtime`.
pub fn store_settings(
    path: &str,
    doc: SettingsDoc,
    expected_mtime: Option<f64>,
) -> Result<StoredSettings, ApiError> {
    settings_doc::write_stored(path, doc, expected_mtime).map_err(update_error)
}

fn update_error(e: SettingsUpdateError) -> ApiError {
    match e {
        SettingsUpdateError::Changed { path, reason } => ApiError::FileChangedError {
            path,
            reason: match reason {
                ChangeReason::Mtime => FileChangeReason::Mtime,
                ChangeReason::Missing => FileChangeReason::Missing,
            },
        },
        SettingsUpdateError::Write { path, reason } => {
            ApiError::SettingsWriteError { path, reason }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn capability_rows_carry_every_cell_of_core() {
        let want: Vec<Value> = capabilities::list_capabilities()
            .iter()
            .map(|c| {
                let per: serde_json::Map<String, Value> = Agent::ALL
                    .into_iter()
                    .map(|a| {
                        let s = c.support(a);
                        let mut cell = json!({ "level": s.level.as_str() });
                        if let Some(n) = s.note {
                            cell["note"] = json!(n);
                        }
                        (a.as_str().to_owned(), cell)
                    })
                    .collect();
                json!({"id": c.id, "group": c.group, "title": c.title, "description": c.description, "perAgent": per})
            })
            .collect();
        assert_eq!(
            serde_json::to_value(capability_rows()).unwrap_or_default(),
            Value::Array(want)
        );
    }

    #[test]
    fn update_errors_keep_path_and_reason() {
        let changed = update_error(SettingsUpdateError::Changed {
            path: "/s".to_owned(),
            reason: ChangeReason::Missing,
        });
        assert_eq!(
            changed,
            ApiError::FileChangedError {
                path: "/s".to_owned(),
                reason: FileChangeReason::Missing
            }
        );
        let refused = check_settings_writable(true).err();
        assert_eq!(refused.as_ref().map(ApiError::status), Some(403));
        assert!(check_settings_writable(false).is_ok());
    }
}
