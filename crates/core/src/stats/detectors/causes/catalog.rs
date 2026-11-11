//! The cause catalog: what each named cause is, its fix, and the memory-file
//! probes that show a rule for it.

use std::sync::LazyLock;

use crate::js::regex::Re;
use crate::stats::schema::CauseId;

/// Where a fix lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CauseKind {
    Rule,
    Env,
    Workflow,
    None,
}

impl CauseKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Env => "env",
            Self::Workflow => "workflow",
            Self::None => "none",
        }
    }
}

/// The calls a cause's rate is taken over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Per {
    Shell,
    RowTool,
    Tools(&'static [&'static str]),
}

/// One probe: every regex must match a memory line or its ±1 window.
pub type ProbeSrc = &'static [(&'static str, &'static str)];

#[derive(Debug)]
pub struct CauseSpec {
    pub kind: CauseKind,
    pub label: &'static str,
    pub fix: Option<&'static str>,
    pub failing: bool,
    pub per: Per,
    pub probes: &'static [ProbeSrc],
    pub advice: Option<&'static str>,
}

const WRITE_TOOL_NAMES: &[&str] = &["Write", "Edit", "MultiEdit", "NotebookEdit"];

/// Claude's tools that write a file.
pub const WRITE_TOOLS: &[&str] = WRITE_TOOL_NAMES;

/// Every agent's file-edit tool names.
pub const FILE_EDIT_TOOLS: &[&str] = &[
    "Write",
    "Edit",
    "MultiEdit",
    "NotebookEdit",
    "edit",
    "write",
    "multiedit",
    "patch",
    "apply_patch",
];

pub const PATH_FILES: &str = "~/.zshenv for zsh (every zsh reads it); ~/.bashrc for bash under Claude Code, whose shell snapshot captures it; other agents' bash reads no rc file, so export PATH before starting them";

const SHELL_GLOB: &str = r"\bglob(?:s|bing)?\b(?!\s+tool)";

pub const GIT_DIFF_FIX: &str =
    "Never print a full-branch git diff: run git diff --stat first, then diff one file at a time.";
pub const GIT_DIFF_PROBES: &[ProbeSrc] = &[&[(r"\bgit diff\b", ""), (r"--stat\b|\bone file\b|\|\s*head\b", "")]];

/// Tools that run a shell, across the four agents.
pub const SHELL_TOOLS: &[&str] = &[
    "Bash",
    "bash",
    "shell",
    "local_shell",
    "run_command",
    "execute_command",
];

pub fn is_shell_tool(tool: &str) -> bool {
    SHELL_TOOLS.contains(&tool)
}

static SPECS: LazyLock<Vec<CauseSpec>> = LazyLock::new(|| {
    let zshenv = "~/.zshenv (every zsh reads it)";
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    vec![
        CauseSpec {
            kind: CauseKind::Rule,
            label: "unquoted glob in a flag value (zsh no matches found)",
            fix: Some("Shell is zsh: single-quote every glob passed as a flag value — grep -rn --include='*.ts' pat ., rg -g '*.ts', find . -name '*.md'. An unmatched bare glob aborts the whole command."),
            failing: true,
            per: Per::Shell,
            probes: &[&[("quot", "i"), ("glob", "i")], &[(r#"--include=['"]"#, "")]],
            advice: Some(leak(format!("make the shell forgiving instead: setopt nonomatch in {zshenv}, or a PreToolUse hook that quotes flag globs"))),
        },
        CauseSpec {
            kind: CauseKind::Rule,
            label: "unmatched path glob (zsh no matches found)",
            fix: Some("zsh: an unmatched glob aborts the command even with 2>/dev/null. List with find or fd, or quote the pattern."),
            failing: true,
            per: Per::Shell,
            probes: &[
                &[(SHELL_GLOB, "i"), (r"\(N\)|nomatch|null_?glob|unmatched|no matches found|path glob|globs? (?:in|for|on) (?:a )?paths?", "i")],
                &[(r"\bzsh\b", "i"), (SHELL_GLOB, "i"), (r"\bls -d\b|\bfind\b|\bfd\b", "")],
            ],
            advice: Some(leak(format!("setopt nonomatch in {zshenv}, or a PreToolUse hook, so an unmatched glob stays a word"))),
        },
        CauseSpec {
            kind: CauseKind::Rule,
            label: "unquoted echo === separator",
            fix: Some("quote the separator: echo \"===\" — zsh expands a bare === to a command name, exits 1, and never runs the rest of the line"),
            failing: true,
            per: Per::Shell,
            probes: &[
                &[(r#"\becho\s+["'`]?="#, "")],
                &[("===", ""), (r"\becho\b|\bseparator|\bzsh\b", "i")],
                &[(r"\b(?:un)?setopt\s+(?:no_?)?equals\b", "i")],
            ],
            advice: Some(leak(format!("unsetopt equals in {zshenv}, so === is a plain word"))),
        },
        CauseSpec {
            kind: CauseKind::Rule,
            label: "pkill -f matched and killed its own shell (exit 144)",
            fix: Some("pkill -f PAT matches the harness shell, whose command line holds the whole call, and kills it (exit 144). Kill by name (pkill -x NAME) or by a saved PID (kill \"$(cat srv.pid)\"). A bracketed '[p]attern' helps only when no other part of the call repeats the plain text: pkill -f '[b]un run dev'; bun run dev & still dies."),
            failing: true,
            per: Per::Shell,
            probes: &[&[("pkill", "")]],
            advice: Some("a PreToolUse hook that rejects pkill -f, pgrep -f | kill and ps | grep | kill in favour of pkill -x NAME or a PID file: a bracketed pattern still kills the shell when the call repeats its plain text"),
        },
        CauseSpec {
            kind: CauseKind::Rule,
            label: "harness blocked sleep N followed by a command",
            fix: Some("Don't sleep N && cmd: wait with an until-loop (until grep -q DONE f; do sleep 2; done), Monitor, or run_in_background."),
            failing: true,
            per: Per::Shell,
            probes: &[&[(r"\bsleep\b", ""), ("Monitor|until|background", "i")]],
            advice: Some("the harness already blocks it: put the wait pattern in the workflow or subagent prompt that keeps hitting it"),
        },
        CauseSpec {
            kind: CauseKind::Env,
            label: "command not found",
            fix: Some(leak(format!("install the missing binaries, or put them on PATH in {PATH_FILES}"))),
            failing: true,
            per: Per::Shell,
            probes: &[],
            advice: Some(leak(format!("install the binaries on this host, or fix PATH ({PATH_FILES}): a memory line cannot add a binary"))),
        },
        CauseSpec {
            kind: CauseKind::Rule,
            label: "shell output too large: persisted to a file and read back",
            fix: Some("Bound shell output before it lands: | head -n 200, or write to a file and Read a range."),
            failing: false,
            per: Per::Shell,
            probes: &[
                &[(r"\|\s*(?:head|tail)\b", ""), (r"\boutput\b", "i")],
                &[
                    (r"\b(?:shell|command|bash|tool|terminal) output\b|\boutput of (?:a |the )?(?:shell|command)", "i"),
                    (r"\bbound|\blimit|\bcap(?:s|ped)?\b|truncat|\bhead\b|\btail\b", "i"),
                ],
            ],
            advice: Some("a PreToolUse hook that pipes long-output commands (cat, grep -r, logs, a bare git diff) through head or adds --stat to git diff, or name the bound in the workflow prompt"),
        },
        CauseSpec {
            kind: CauseKind::Rule,
            label: "Write/Edit before the file was read",
            fix: Some("Read a file with the Read tool before Write/Edit: cat or sed in Bash does not count as a read."),
            failing: true,
            per: Per::Tools(WRITE_TOOL_NAMES),
            probes: &[&[("Read tool", "i"), (r"\b(?:before|first)\b", "i"), (r"\b(?:Edit|Write)\b", "")]],
            advice: Some("put the Read-first step in the workflow or subagent prompt that writes files"),
        },
        CauseSpec {
            kind: CauseKind::Rule,
            label: "Write/Edit after the file changed since it was read",
            fix: Some("Re-Read a file with the Read tool after a formatter, linter or Bash command touched it, then Write/Edit."),
            failing: true,
            per: Per::Tools(WRITE_TOOL_NAMES),
            probes: &[&[("re-?read", "i"), ("format|lint|modified|changed|touch", "i")]],
            advice: Some("put the re-Read step in the workflow or subagent prompt that formats then writes"),
        },
        CauseSpec {
            kind: CauseKind::Rule,
            label: "Edit old_string not found, or not unique",
            fix: Some("Copy old_string from a fresh Read of the exact lines, and widen it until it is unique."),
            failing: true,
            per: Per::Tools(&["Edit", "MultiEdit"]),
            probes: &[&[("old_string", "")], &[(r"\bEdit\b", ""), (r"\bunique\b|exact (?:lines|text)", "i")]],
            advice: Some("put the fresh-Read step in the workflow or subagent prompt that edits"),
        },
        CauseSpec {
            kind: CauseKind::Workflow,
            label: "StructuredOutput did not match its schema",
            fix: Some("Name every required field in the workflow or subagent prompt that asks for this output."),
            failing: true,
            per: Per::Tools(&["StructuredOutput"]),
            probes: &[],
            advice: None,
        },
        CauseSpec {
            kind: CauseKind::Rule,
            label: "harness dangerous-rm guard",
            fix: Some("rm absolute paths, never $VAR/* or a path behind cd: write rm -rf -- \"${D:?}\"/x so an empty variable cannot widen the target."),
            failing: true,
            per: Per::Shell,
            probes: &[&[(r"\brm\b", ""), (r":\?|absolute", "")]],
            advice: Some("a PreToolUse hook that rejects rm with a $VAR or cd'd target"),
        },
        CauseSpec {
            kind: CauseKind::Rule,
            label: "scratch script cannot resolve the project's modules",
            fix: Some("Run scratch scripts that import project packages from inside the project (bun -e, or a file under the repo), not from /tmp: modules resolve from the script's own directory."),
            failing: true,
            per: Per::Shell,
            probes: &[&[(r"scratch|TMPDIR|\/tmp", "i"), (r"\bimports?\b|\bmodules?\b|resolv", "i")]],
            advice: Some("put the scratch-dir rule in the workflow or subagent prompt that writes scripts"),
        },
        CauseSpec {
            kind: CauseKind::Env,
            label: "grep is ugrep: pattern exceeds its complexity limit",
            fix: Some("grep is aliased to ugrep, which rejects long alternations: use rg or command grep for big patterns."),
            failing: true,
            per: Per::Shell,
            probes: &[&[("ugrep|command grep", "")]],
            advice: Some("unalias grep in the shell Claude Code snapshots, or install rg and say so in the prompt"),
        },
        CauseSpec {
            kind: CauseKind::None,
            label: "errored, no named cause",
            fix: None,
            failing: true,
            per: Per::RowTool,
            probes: &[],
            advice: None,
        },
    ]
});

static PROBES: LazyLock<Vec<Vec<Vec<Re>>>> = LazyLock::new(|| {
    SPECS
        .iter()
        .map(|s| compile_probes(s.probes))
        .collect()
});

static GIT_DIFF: LazyLock<Vec<Vec<Re>>> = LazyLock::new(|| compile_probes(GIT_DIFF_PROBES));

fn compile_probes(probes: &[ProbeSrc]) -> Vec<Vec<Re>> {
    probes
        .iter()
        .map(|p| p.iter().map(|(src, flags)| Re::new(src, flags)).collect())
        .collect()
}

fn index(id: CauseId) -> usize {
    CauseId::ALL.iter().position(|c| *c == id).unwrap_or(0)
}

/// The catalog entry for a cause.
pub fn spec(id: CauseId) -> &'static CauseSpec {
    &SPECS[index(id)]
}

/// The compiled rule probes for a cause.
pub fn probes(id: CauseId) -> &'static [Vec<Re>] {
    &PROBES[index(id)]
}

pub fn git_diff_probes() -> &'static [Vec<Re>] {
    &GIT_DIFF
}
