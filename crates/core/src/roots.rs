//! Where Claude transcripts live: the default root from the environment plus
//! extra roots from `settings.json`. Pure: the caller passes env and file text.

use std::borrow::Cow;

use serde_json::Value;

use crate::js::{json, text};
use crate::model::Agent;
use crate::paths::PathStyle;

/// The environment inputs root resolution reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RootsEnv {
    /// The user's home dir (`os.homedir()`).
    pub home: String,
    /// `CLAUDE_CONFIG_DIR`, a list split on the platform's `PATH` delimiter;
    /// the first entry wins.
    pub claude_config_dir: Option<String>,
    /// `PEEKTRACE_CLAUDE_PROJECTS`: the default projects root, used verbatim.
    pub claude_projects: Option<String>,
    /// `PEEKTRACE_DIR`: where `settings.json` lives.
    pub peektrace_dir: Option<String>,
    /// How paths join on this platform.
    pub style: PathStyle,
}

impl RootsEnv {
    /// Read the variables through `var`, so tests need no process env.
    pub fn from_vars(home: String, style: PathStyle, var: impl Fn(&str) -> Option<String>) -> Self {
        Self {
            home,
            claude_config_dir: var("CLAUDE_CONFIG_DIR"),
            claude_projects: var("PEEKTRACE_CLAUDE_PROJECTS"),
            peektrace_dir: var("PEEKTRACE_DIR"),
            style,
        }
    }

    /// `$PEEKTRACE_DIR/settings.json`, else `~/.peektrace/settings.json`.
    pub fn settings_path(&self) -> String {
        let dir = self
            .peektrace_dir
            .clone()
            .unwrap_or_else(|| self.style.join(&[&self.home, ".peektrace"]));
        self.style.join(&[&dir, "settings.json"])
    }
}

/// One extra root a user declares in `settings.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootEntry {
    pub path: String,
    /// Transcript text (see [`text::units`]).
    pub label: Option<String>,
}

/// One root scanned for transcripts. `id` and `label` are transcript text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub label: String,
    pub projects_root: String,
}

/// A root entry when `v` is an object with a string `path` and an optional
/// string `label`. `escaped` says the strings are already transcript text;
/// the path then drops to plain text, as Node's `fs` sees it.
fn root_entry(v: &Value, escaped: bool) -> Option<RootEntry> {
    let obj = v.as_object()?;
    let path = obj.get("path")?.as_str()?;
    let label = match obj.get("label") {
        None => None,
        Some(l) => Some(l.as_str()?),
    };
    let (path, label) = if escaped {
        (
            text::to_display(path).into_owned(),
            label.map(str::to_owned),
        )
    } else {
        (
            path.to_owned(),
            label.map(|l| text::escape_reserved(l).into_owned()),
        )
    };
    Some(RootEntry { path, label })
}

/// What root resolution takes from `settings.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// Claude's extra roots.
    pub claude: Vec<RootEntry>,
    /// The file has text but yields no `roots`; the caller warns.
    pub unparseable: bool,
}

/// Valid entries of one agent's root list, `None` when it is not a list.
fn entries_of(v: &Value, escaped: bool) -> Option<Vec<RootEntry>> {
    v.as_array()
        .map(|list| list.iter().filter_map(|e| root_entry(e, escaped)).collect())
}

/// True when `roots` decodes whole: an object whose known agents each map
/// to a list of valid entries.
fn roots_decode(roots: &Value) -> bool {
    roots.as_object().is_some_and(|map| {
        Agent::ALL
            .into_iter()
            .filter_map(|a| map.get(a.as_str()))
            .all(|v| {
                v.as_array()
                    .is_some_and(|list| list.iter().all(|e| root_entry(e, false).is_some()))
            })
    })
}

/// True when some known agent keeps at least one valid entry.
fn roots_salvage(roots: &Value) -> bool {
    roots.as_object().is_some_and(|map| {
        Agent::ALL
            .into_iter()
            .filter_map(|a| map.get(a.as_str()).and_then(|v| entries_of(v, false)))
            .any(|entries| !entries.is_empty())
    })
}

/// Settings text as a JSON value, read as leniently as `JSON.parse`. The
/// flag says strings are transcript text.
fn parse_json(raw: &str) -> Option<(Value, bool)> {
    if let Ok(v) = serde_json::from_str::<Value>(raw) {
        return Some((v, false));
    }
    let fixed = json::repair(raw, json::Overflow::Zero)?;
    serde_json::from_str(&fixed).ok().map(|v| (v, true))
}

/// Parse `settings.json` text. Tolerant: bad JSON yields no roots, and each
/// invalid entry is dropped on its own. Non-blank text that decodes to no
/// `roots`, even after dropping bad entries, is `unparseable`.
pub fn parse_settings(raw: &str) -> Settings {
    let trimmed = text::trim(raw);
    if trimmed.is_empty() {
        return Settings::default();
    }
    let Some((json, escaped)) = parse_json(trimmed) else {
        return Settings {
            claude: Vec::new(),
            unparseable: true,
        };
    };
    let roots = json.as_object().and_then(|o| o.get("roots"));
    let has_roots = roots.is_some_and(|r| roots_decode(r) || roots_salvage(r));
    let claude = roots
        .and_then(Value::as_object)
        .and_then(|r| r.get(Agent::Claude.as_str()))
        .and_then(|v| entries_of(v, escaped))
        .unwrap_or_default();
    Settings {
        claude,
        unparseable: !has_roots,
    }
}

/// First non-empty, trimmed entry of a `PATH`-style list. Splitting on `:`
/// everywhere, as TS does, would cut a Windows drive letter off.
fn first_dir(value: Option<&str>, style: PathStyle) -> Option<&str> {
    value?
        .split(style.delimiter())
        .map(text::trim)
        .find(|p| !p.is_empty())
}

/// Expand a leading `~` or `~/` against `home`. `~\\` stays as is, as in TS.
fn expand_home(path: &str, home: &str, style: PathStyle) -> String {
    if path == "~" {
        return home.to_owned();
    }
    path.strip_prefix("~/")
        .map_or_else(|| path.to_owned(), |rest| style.join(&[home, rest]))
}

/// Every Claude root to scan: the default first, then each configured extra,
/// skipping roots already seen (the default wins) and giving each a unique
/// label.
pub fn claude_sources(env: &RootsEnv, extras: &[RootEntry]) -> Vec<Source> {
    let claude_home = first_dir(env.claude_config_dir.as_deref(), env.style)
        .map_or_else(|| env.style.join(&[&env.home, ".claude"]), str::to_owned);
    let default_root = env
        .claude_projects
        .clone()
        .unwrap_or_else(|| env.style.join(&[&claude_home, "projects"]));
    let default = Source {
        id: "claude".to_owned(),
        label: "default".to_owned(),
        projects_root: default_root,
    };
    extras.iter().fold(vec![default], |mut sources, entry| {
        let home = expand_home(text::trim(&entry.path), &env.home, env.style);
        if home.is_empty() {
            return sources;
        }
        let projects_root = env.style.join(&[&home, "projects"]);
        if sources.iter().any(|s| s.projects_root == projects_root) {
            return sources;
        }
        let base = [
            entry.label.as_deref().map(|l| Cow::Borrowed(text::trim(l))),
            Some(text::escape_reserved(env.style.basename(&home))),
        ]
        .into_iter()
        .flatten()
        .find(|l| !l.is_empty())
        .map_or_else(|| "source".to_owned(), Cow::into_owned);
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
    fn windows_roots() {
        let e = RootsEnv {
            home: "C:\\Users\\u".to_owned(),
            style: PathStyle::Windows,
            ..RootsEnv::default()
        };
        assert_eq!(e.settings_path(), "C:\\Users\\u\\.peektrace\\settings.json");
        let s = claude_sources(
            &e,
            &[entry("~/work/.claude", None), entry("D:/x/", Some("x"))],
        );
        let roots: Vec<_> = s.iter().map(|s| s.projects_root.as_str()).collect();
        assert_eq!(
            roots,
            [
                "C:\\Users\\u\\.claude\\projects",
                "C:\\Users\\u\\work\\.claude\\projects",
                "D:\\x\\projects",
            ]
        );
        assert_eq!(s[1].label, ".claude");
        let e = RootsEnv {
            claude_config_dir: Some(" ; C:\\x\\.claude;D:\\y".to_owned()),
            ..e
        };
        assert_eq!(
            claude_sources(&e, &[])[0].projects_root,
            "C:\\x\\.claude\\projects"
        );
    }

    #[test]
    fn settings_with_lone_surrogates_keep_roots() {
        let s = parse_settings(
            "{\"roots\":{\"claude\":[{\"path\":\"/w\\ud800\",\"label\":\"w\\ud800\"},\
             {\"path\":\"/x\u{10F801}\",\"label\":\"x\u{10F801}\"}]}}",
        );
        assert!(!s.unparseable);
        assert_eq!(
            s.claude,
            [
                entry("/w\u{FFFD}", Some("w\u{10F800}")),
                entry("/x\u{10F801}", Some("x\u{10F7FF}\u{10F801}")),
            ]
        );
        let strict = parse_settings(
            "{\"roots\":{\"claude\":[{\"path\":\"/x\",\"label\":\"x\u{10F801}\"}]}}",
        );
        assert_eq!(strict.claude, [entry("/x", Some("x\u{10F7FF}\u{10F801}"))]);
        let big =
            parse_settings(r#"{"n":1e400,"roots":{"claude":[{"path":1e400},{"path":"/a"}]}}"#);
        assert_eq!(big.claude, [entry("/a", None)]);
    }

    #[test]
    fn settings_parse_is_tolerant() {
        let raw = r#"{"roots":{"claude":[{"path":"~/a","label":"x"},{"path":1},{"path":"/b","label":null},{"path":"/c"}],"codex":5}}"#;
        assert_eq!(
            parse_settings(raw),
            Settings {
                claude: vec![entry("~/a", Some("x")), entry("/c", None)],
                unparseable: false,
            }
        );
        assert_eq!(parse_settings(r#"{"roots":{"claude":{}}}"#).claude, []);
        assert_eq!(parse_settings(""), Settings::default());
    }

    /// Which inputs make TS log `ignoring unparseable settings`.
    #[test]
    fn unparseable_settings_match_ts() {
        let warns = [
            "not json",
            "{}",
            "[]",
            "5",
            r#"[{"roots":{}}]"#,
            r#"{"roots":null}"#,
            r#"{"roots":[]}"#,
            r#"{"roots":{"claude":null}}"#,
            r#"{"roots":{"codex":5}}"#,
            r#"{"roots":{"claude":[{"path":1}]}}"#,
            r#"{"roots":{"claude":[{"path":"/a","label":null}]}}"#,
        ];
        let quiet = [
            "",
            " \n ",
            r#"{"roots":{}}"#,
            r#"{"roots":{"foo":1}}"#,
            r#"{"roots":{"claude":[]}}"#,
            r#"{"roots":{"claude":[{"path":"/a"}],"codex":5}}"#,
            r#"{"roots":{"pi":[{"path":"/a"}]}}"#,
        ];
        for raw in warns {
            assert!(parse_settings(raw).unparseable, "{raw}");
        }
        for raw in quiet {
            assert!(!parse_settings(raw).unparseable, "{raw}");
        }
    }
}
