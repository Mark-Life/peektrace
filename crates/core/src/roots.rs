//! Where Claude transcripts live: the default root from the environment plus
//! extra roots from `settings.json`. Pure: the caller passes env and file text.

use std::borrow::Cow;

use crate::js::text;
use crate::model::Agent;
use crate::paths::PathStyle;
use crate::settings_doc::{DocEntry, settings_doc};

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
    /// `CODEX_HOME`, else `~/.codex`.
    pub codex_home: Option<String>,
    /// `PEEKTRACE_CODEX_SESSIONS`: the Codex sessions root, used verbatim.
    pub codex_sessions: Option<String>,
    /// `PEEKTRACE_PI_SESSIONS`: the Pi sessions root, used verbatim.
    pub pi_sessions: Option<String>,
    /// `PEEKTRACE_OPENCODE_DATA`: the opencode data dir, when non-empty.
    pub opencode_data: Option<String>,
    /// `XDG_DATA_HOME`, the parent of opencode's data dir when non-empty.
    pub xdg_data_home: Option<String>,
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
            codex_home: var("CODEX_HOME"),
            codex_sessions: var("PEEKTRACE_CODEX_SESSIONS"),
            pi_sessions: var("PEEKTRACE_PI_SESSIONS"),
            opencode_data: var("PEEKTRACE_OPENCODE_DATA"),
            xdg_data_home: var("XDG_DATA_HOME"),
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
    /// The agent's config dir for this root (`~/.claude` for the default).
    pub home: String,
    pub projects_root: String,
}

/// A document entry as root resolution uses it: the path as plain text,
/// as Node's `fs` sees it; the label stays transcript text.
fn root_entry(e: &DocEntry) -> RootEntry {
    RootEntry {
        path: text::to_display(&e.path).into_owned(),
        label: e.label.clone(),
    }
}

/// What root resolution takes from `settings.json`: each agent's extra
/// roots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub claude: Vec<RootEntry>,
    pub codex: Vec<RootEntry>,
    pub pi: Vec<RootEntry>,
    pub opencode: Vec<RootEntry>,
    /// The file has text but yields no `roots`; the caller warns.
    pub unparseable: bool,
}

impl Settings {
    /// One agent's extra roots.
    pub fn extras(&self, agent: Agent) -> &[RootEntry] {
        match agent {
            Agent::Claude => &self.claude,
            Agent::Codex => &self.codex,
            Agent::Pi => &self.pi,
            Agent::Opencode => &self.opencode,
        }
    }
}

/// Parse `settings.json` text through the settings editor's decoder.
/// Tolerant: bad JSON yields no roots, and each invalid entry is dropped on
/// its own. Non-blank text that decodes to no `roots`, even after dropping
/// bad entries, is `unparseable`.
pub fn parse_settings(raw: &str) -> Settings {
    if text::trim(raw).is_empty() {
        return Settings::default();
    }
    let doc = settings_doc(raw);
    let of = |agent: Agent| -> Vec<RootEntry> {
        doc.roots
            .iter()
            .flatten()
            .filter(|(a, _)| *a == agent)
            .flat_map(|(_, entries)| entries.iter().map(root_entry))
            .collect()
    };
    Settings {
        claude: of(Agent::Claude),
        codex: of(Agent::Codex),
        pi: of(Agent::Pi),
        opencode: of(Agent::Opencode),
        unparseable: doc.roots.is_none(),
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

/// An agent's sessions root under one of its config dirs.
fn projects_root_in(agent: Agent, home: &str, style: PathStyle) -> String {
    match agent {
        Agent::Claude => style.join(&[home, "projects"]),
        Agent::Codex => style.join(&[home, "sessions"]),
        Agent::Pi => style.join(&[home, "agent", "sessions"]),
        Agent::Opencode => home.to_owned(),
    }
}

/// `opencode`'s data dir: `PEEKTRACE_OPENCODE_DATA`, else under
/// `XDG_DATA_HOME`, else `~/.local/share/opencode`.
fn opencode_data(env: &RootsEnv) -> String {
    let non_empty = |v: &Option<String>| v.clone().filter(|s| !s.is_empty());
    non_empty(&env.opencode_data).unwrap_or_else(|| {
        non_empty(&env.xdg_data_home).map_or_else(
            || env.style.join(&[&env.home, ".local", "share", "opencode"]),
            |x| env.style.join(&[&x, "opencode"]),
        )
    })
}

/// An agent's root from the environment alone. `CODEX_HOME` is used
/// verbatim, `""` included.
fn default_source(agent: Agent, env: &RootsEnv) -> Source {
    let join = |parts: &[&str]| env.style.join(parts);
    let under = |home: String, root: &Option<String>| {
        let root = root
            .clone()
            .unwrap_or_else(|| projects_root_in(agent, &home, env.style));
        (home, root)
    };
    let (home, projects_root) = match agent {
        Agent::Claude => under(
            first_dir(env.claude_config_dir.as_deref(), env.style)
                .map_or_else(|| join(&[&env.home, ".claude"]), str::to_owned),
            &env.claude_projects,
        ),
        Agent::Codex => under(
            env.codex_home
                .clone()
                .unwrap_or_else(|| join(&[&env.home, ".codex"])),
            &env.codex_sessions,
        ),
        Agent::Pi => under(join(&[&env.home, ".pi"]), &env.pi_sessions),
        Agent::Opencode => under(opencode_data(env), &None),
    };
    Source {
        id: agent.as_str().to_owned(),
        label: "default".to_owned(),
        home,
        projects_root,
    }
}

/// Every Claude root to scan; see [`sources`].
pub fn claude_sources(env: &RootsEnv, extras: &[RootEntry]) -> Vec<Source> {
    sources(Agent::Claude, env, extras)
}

/// Every root of `agent` to scan: the default first, then each configured
/// extra, skipping roots already seen (the default wins) and giving each a
/// unique label.
pub fn sources(agent: Agent, env: &RootsEnv, extras: &[RootEntry]) -> Vec<Source> {
    let default = default_source(agent, env);
    extras.iter().fold(vec![default], |mut sources, entry| {
        let home = expand_home(text::trim(&entry.path), &env.home, env.style);
        if home.is_empty() {
            return sources;
        }
        let projects_root = projects_root_in(agent, &home, env.style);
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
            id: format!("{agent}:{label}"),
            label,
            home,
            projects_root,
        });
        sources
    })
}

#[cfg(test)]
mod tests;
