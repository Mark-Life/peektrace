//! The memory-file layout the rule tests read: a user home with symlinked
//! global files, an import, project files per agent, and a Pi home.

use peektrace_core::Agent;
use peektrace_core::js::time::{Utc, parse_date};
use peektrace_core::roots::Source;
use peektrace_core::stats::detectors::rules::{CodexCover, MemoryDoc, RuleCover};
use peektrace_core::stats::memory_files::{AgentHomes, read_memory_docs};

use super::{TempDir, set_mtime};

pub const SECRET: &str = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";

pub fn ms(iso: &str) -> f64 {
    #[allow(clippy::cast_precision_loss, reason = "epoch ms fit in f64")]
    let v = parse_date(iso, &Utc).expect("date") as f64;
    v
}

/// The TS `beforeAll` layout under `<base>/u`.
pub fn fixture() -> TempDir {
    let t = TempDir::new("rules");
    for d in [
        "u/.agents/skills",
        "u/.claude",
        "u/.codex",
        "u/proj/app",
        "u/proj/.git",
    ] {
        t.mkdir(d);
    }
    let shared = t.write(
        "u/.agents/AGENTS.md",
        &[
            "# Shell",
            &format!("- Token for the bot: {SECRET}"),
            "- Quote separator echoes: `echo \"===\"`, never bare `echo ===`.",
            "- Quote globs passed as flag values: `grep -rn --include='*.ts' x .`.",
            "- Never `sleep N` then `tail` to wait on a job. Call Monitor.",
            "@~/.agents/skills/rm.md",
            "Example only: `@~/.agents/skills/missing.md`",
        ]
        .join("\n"),
    );
    t.write(
        "u/.agents/skills/rm.md",
        "- rm absolute paths only, never a bare $VAR target\n",
    );
    set_mtime(&shared, 1_785_585_600_000); // 2026-08-01T12:00:00Z
    std::os::unix::fs::symlink(&shared, t.join("u/.claude/CLAUDE.md")).expect("symlink");
    std::os::unix::fs::symlink(&shared, t.join("u/.codex/AGENTS.md")).expect("symlink");
    t.write(
        "u/proj/AGENTS.md",
        "- pkill: never chain it.\n- This host has no jq: use bun -e instead of jq.",
    );
    t.write(
        "u/proj/app/CLAUDE.local.md",
        "- Never install `node_modules` or build in `/tmp`.",
    );
    t.write(
        "u/proj/app/AGENTS.override.md",
        "- Override for Codex and Pi.",
    );
    t.write("u/proj/app/AGENTS.md", "- Shadowed by the override.");
    t.write("u/.pi/agent/AGENTS.md", "- Pi global rules.");
    t.write("u/.pi/agent/CLAUDE.md", "- Shadowed for Pi.");
    t
}

pub fn source(id: &str, home: String, projects: String) -> Source {
    Source {
        id: id.to_owned(),
        label: id.to_owned(),
        home,
        projects_root: projects,
    }
}

pub fn read_with(
    t: &TempDir,
    claude: &[Source],
    dirs: &[String],
    scope: &[Agent],
) -> Vec<MemoryDoc> {
    let homes = AgentHomes {
        claude,
        codex_home: t.join("u/.codex"),
        pi_home: t.join("u/.pi"),
    };
    read_memory_docs(&homes, scope, dirs)
}

pub fn read_docs(
    t: &TempDir,
    dirs: &[String],
    claude_dir: &str,
    scope: &[Agent],
) -> Vec<MemoryDoc> {
    let claude = [source(
        "claude",
        t.join(&format!("u/{claude_dir}")),
        t.join("u/projects"),
    )];
    read_with(t, &claude, dirs, scope)
}

pub const CLAUDE_CODEX: [Agent; 2] = [Agent::Claude, Agent::Codex];

pub fn cover(agent: Agent, dir: Option<&str>) -> RuleCover {
    RuleCover {
        agent,
        codex: None,
        dir: dir.map(str::to_owned),
        source: None,
    }
}

pub fn codex_cover(dir: &str, exact: bool, stops: &[String]) -> RuleCover {
    RuleCover {
        codex: Some(CodexCover {
            exact,
            stops: stops.to_vec(),
        }),
        ..cover(Agent::Codex, Some(dir))
    }
}
