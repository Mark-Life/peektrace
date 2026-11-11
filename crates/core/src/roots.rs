//! Where Claude transcripts live: the default root from the environment plus
//! extra roots from `settings.json`. Pure: the caller passes env and file text.

use serde_json::Value;

use crate::js::text;
use crate::paths;

/// The environment inputs root resolution reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RootsEnv {
    /// The user's home dir (`os.homedir()`).
    pub home: String,
    /// `CLAUDE_CONFIG_DIR`, a `:`-separated list; the first entry wins.
    pub claude_config_dir: Option<String>,
    /// `PEEKTRACE_CLAUDE_PROJECTS`: the default projects root, used verbatim.
    pub claude_projects: Option<String>,
    /// `PEEKTRACE_DIR`: where `settings.json` lives.
    pub peektrace_dir: Option<String>,
}

impl RootsEnv {
    /// Read the variables through `var`, so tests need no process env.
    pub fn from_vars(home: String, var: impl Fn(&str) -> Option<String>) -> Self {
        Self {
            home,
            claude_config_dir: var("CLAUDE_CONFIG_DIR"),
            claude_projects: var("PEEKTRACE_CLAUDE_PROJECTS"),
            peektrace_dir: var("PEEKTRACE_DIR"),
        }
    }

    /// `$PEEKTRACE_DIR/settings.json`, else `~/.peektrace/settings.json`.
    pub fn settings_path(&self) -> String {
        let dir = self
            .peektrace_dir
            .clone()
            .unwrap_or_else(|| paths::join(&[&self.home, ".peektrace"]));
        paths::join(&[&dir, "settings.json"])
    }
}

/// One extra root a user declares in `settings.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootEntry {
    pub path: String,
    pub label: Option<String>,
}

/// One root scanned for transcripts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub label: String,
    pub projects_root: String,
}

/// A root entry when `v` is an object with a string `path` and an optional
/// string `label`.
fn root_entry(v: &Value) -> Option<RootEntry> {
    let obj = v.as_object()?;
    let path = obj.get("path")?.as_str()?.to_owned();
    let label = match obj.get("label") {
        None => None,
        Some(l) => Some(l.as_str()?.to_owned()),
    };
    Some(RootEntry { path, label })
}

/// Claude's extra roots from `settings.json` text. Tolerant: bad JSON yields
/// none, and each invalid entry is dropped on its own.
pub fn parse_claude_roots(raw: &str) -> Vec<RootEntry> {
    let trimmed = text::trim(raw);
    if trimmed.is_empty() {
        return Vec::new();
    }
    let Ok(json) = serde_json::from_str::<Value>(trimmed) else {
        return Vec::new();
    };
    json.get("roots")
        .and_then(Value::as_object)
        .and_then(|roots| roots.get("claude"))
        .and_then(Value::as_array)
        .map(|entries| entries.iter().filter_map(root_entry).collect())
        .unwrap_or_default()
}

/// First non-empty, trimmed entry of a `:`-separated list.
fn first_dir(value: Option<&str>) -> Option<&str> {
    value?.split(':').map(text::trim).find(|p| !p.is_empty())
}

/// Expand a leading `~` or `~/` against `home`.
fn expand_home(path: &str, home: &str) -> String {
    if path == "~" {
        return home.to_owned();
    }
    path.strip_prefix("~/")
        .map_or_else(|| path.to_owned(), |rest| paths::join(&[home, rest]))
}

/// Every Claude root to scan: the default first, then each configured extra,
/// skipping roots already seen (the default wins) and giving each a unique
/// label.
pub fn claude_sources(env: &RootsEnv, extras: &[RootEntry]) -> Vec<Source> {
    let claude_home = first_dir(env.claude_config_dir.as_deref())
        .map_or_else(|| paths::join(&[&env.home, ".claude"]), str::to_owned);
    let default_root = env
        .claude_projects
        .clone()
        .unwrap_or_else(|| paths::join(&[&claude_home, "projects"]));
    let default = Source {
        id: "claude".to_owned(),
        label: "default".to_owned(),
        projects_root: default_root,
    };
    extras.iter().fold(vec![default], |mut sources, entry| {
        let home = expand_home(text::trim(&entry.path), &env.home);
        if home.is_empty() {
            return sources;
        }
        let projects_root = paths::join(&[&home, "projects"]);
        if sources.iter().any(|s| s.projects_root == projects_root) {
            return sources;
        }
        let base = [
            entry.label.as_deref().map(text::trim),
            Some(paths::basename(&home)),
        ]
        .into_iter()
        .flatten()
        .find(|l| !l.is_empty())
        .unwrap_or("source")
        .to_owned();
        let taken = |l: &str| l == "default" || sources.iter().any(|s| s.label == l);
        let label = if taken(&base) {
            (2..=sources.len() + 2)
                .map(|n| format!("{base}-{n}"))
                .find(|l| !taken(l))
                .unwrap_or_default()
        } else {
            base
        };
        sources.push(Source {
            id: format!("claude:{label}"),
            label,
            projects_root,
        });
        sources
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> RootsEnv {
        RootsEnv {
            home: "/home/u".to_owned(),
            ..RootsEnv::default()
        }
    }

    fn entry(path: &str, label: Option<&str>) -> RootEntry {
        RootEntry {
            path: path.to_owned(),
            label: label.map(str::to_owned),
        }
    }

    #[test]
    fn default_root_from_home() {
        let s = claude_sources(&env(), &[]);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].projects_root, "/home/u/.claude/projects");
    }

    #[test]
    fn config_dir_and_override() {
        let e = RootsEnv {
            claude_config_dir: Some(" : /c/one/ :/c/two".to_owned()),
            ..env()
        };
        assert_eq!(claude_sources(&e, &[])[0].projects_root, "/c/one/projects");
        let e = RootsEnv {
            claude_projects: Some("/p//x/".to_owned()),
            ..e
        };
        assert_eq!(claude_sources(&e, &[])[0].projects_root, "/p//x/");
    }

    #[test]
    fn extras_dedup_and_label() {
        let extras = [
            entry("~/.claude", None),
            entry(" ~/work/.claude ", Some(" work ")),
            entry("/x/default", None),
            entry("/y/work", None),
            entry("/z/work", None),
            entry("  ", Some("blank")),
            entry("/", None),
        ];
        let s = claude_sources(&env(), &extras);
        let got: Vec<_> = s
            .iter()
            .map(|s| (s.id.as_str(), s.projects_root.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("claude", "/home/u/.claude/projects"),
                ("claude:work", "/home/u/work/.claude/projects"),
                ("claude:default-2", "/x/default/projects"),
                ("claude:work-2", "/y/work/projects"),
                ("claude:work-3", "/z/work/projects"),
                ("claude:source", "/projects"),
            ]
        );
    }

    #[test]
    fn settings_parse_is_tolerant() {
        let raw = r#"{"roots":{"claude":[{"path":"~/a","label":"x"},{"path":1},{"path":"/b","label":null},{"path":"/c"}],"codex":5}}"#;
        assert_eq!(
            parse_claude_roots(raw),
            [entry("~/a", Some("x")), entry("/c", None)]
        );
        assert_eq!(parse_claude_roots("not json"), Vec::new());
        assert_eq!(parse_claude_roots(r#"{"roots":{"claude":{}}}"#), Vec::new());
        assert_eq!(parse_claude_roots(""), Vec::new());
    }
}
