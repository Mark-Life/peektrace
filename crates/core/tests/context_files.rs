//! On-disk context files Claude Code loads for a cwd, and session lookup.
//! Ported from `context-files` and `resolve-context` TS tests. The trees
//! sit under the system temp dir; ancestor dirs above it are filtered out.
//! Paths are built as POSIX strings, so the tree tests are unix only.
#![allow(clippy::float_cmp)]

mod common;

use common::{TempDir, sources};
use peektrace_core::sessions::context_files::{
    cap_memory, gather_on_disk_context_files, rule_globs,
};
use peektrace_core::sessions::model::{ContextScope, OnDiskContextFile};
use peektrace_core::sessions::resolve::{SessionError, find_subagents, resolve_claude_session};

fn encode_slug(p: &str) -> String {
    p.replace(['/', '.'], "-")
}

fn units_len(s: &str) -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "short strings")]
    let n = s.encode_utf16().count() as f64;
    n
}

/// The `context-files` tree: a repo with rules and CLAUDE files at several
/// levels, an AGENTS-only repo, nested and outside worktrees, and memory.
struct Tree {
    dir: TempDir,
}

impl Tree {
    fn base(&self) -> String {
        self.dir.str()
    }
    fn p(&self, rel: &str) -> String {
        format!("{}/{rel}", self.base())
    }
    fn home(&self) -> String {
        self.p("top/user/.claude")
    }
    fn projects(&self) -> String {
        self.p("top/user/.claude/projects")
    }
    fn repo(&self) -> String {
        self.p("top/user/repo")
    }
    fn cwd(&self) -> String {
        self.p("top/user/repo/pkg")
    }
    fn main_repo(&self) -> String {
        self.p("top/user/main")
    }
    fn worktree(&self) -> String {
        self.p("top/user/main/.trees/feat")
    }
    fn outside_tree(&self) -> String {
        self.p("elsewhere/feat")
    }
    fn spaced_repo(&self) -> String {
        self.p("my_app dir")
    }
    fn mem_path(&self, cwd: &str) -> String {
        format!("{}/{}/memory/MEMORY.md", self.projects(), encode_slug(cwd))
    }

    fn put(&self, path: &str, body: &str) {
        let rel = path
            .strip_prefix(&format!("{}/", self.base()))
            .expect("inside the tree");
        self.dir.write(rel, body);
    }

    fn new() -> Self {
        let t = Self {
            dir: TempDir::new("ctxfiles"),
        };
        let (home, repo, cwd) = (t.home(), t.repo(), t.cwd());
        t.put(&format!("{home}/CLAUDE.md"), "global rules\n");
        t.put(&format!("{home}/rules/style.md"), "user rule\n");
        t.put(
            &format!("{home}/rules/scoped.md"),
            "---\npaths: src/**/*.ts\n---\nonly for ts\n",
        );
        t.put(&format!("{repo}/.claude/rules/b.md"), "rule b\n");
        t.put(
            &format!("{repo}/.claude/rules/a/nested.md"),
            "nested rule\n",
        );
        t.put(&format!("{repo}/.claude/rules/notes.txt"), "not a rule\n");
        t.put(
            &format!("{repo}/.claude/rules/scoped.md"),
            "---\npaths:\n  - lib/**\n---\nonly for lib\n",
        );
        t.put(
            &format!("{repo}/.claude/rules/everywhere.md"),
            "---\npaths: \"**\"\n---\nalways\n",
        );
        t.put(&t.p("top/CLAUDE.local.md"), "above home\n");
        t.put(&format!("{repo}/CLAUDE.md"), "parent dir rules\n");
        t.put(&format!("{repo}/CLAUDE.local.md"), "parent local rules\n");
        t.put(&format!("{cwd}/CLAUDE.md"), "cwd rules\n");
        t.put(&format!("{cwd}/CLAUDE.local.md"), "cwd local rules\n");
        t.put(&format!("{cwd}/AGENTS.md"), &"x".repeat(4000));
        t.put(&t.p("agents-only/AGENTS.md"), "agents rules\n");
        t.put(&t.p("agents-only/app/.claude/AGENTS.md"), "nested agents\n");
        let main = t.main_repo();
        t.dir.mkdir("top/user/main/.git/worktrees/feat");
        t.put(&format!("{main}/CLAUDE.md"), "main checkout rules\n");
        t.put(&format!("{main}/CLAUDE.local.md"), "main local rules\n");
        t.put(&format!("{main}/.trees/CLAUDE.md"), "between rules\n");
        t.put(
            &format!("{}/.git", t.worktree()),
            "gitdir: ../../.git/worktrees/feat\n",
        );
        t.put(&format!("{}/CLAUDE.md", t.worktree()), "worktree rules\n");
        t.put(&format!("{main}/.claude/rules/main.md"), "main rule\n");
        t.put(&t.mem_path(&main), "- main memory\n");
        t.dir.mkdir("elsewhere/main/.git/worktrees/feat");
        t.put(
            &format!("{}/.git", t.outside_tree()),
            "gitdir: ../main/.git/worktrees/feat\n",
        );
        t.put(&t.mem_path(&t.p("elsewhere/main")), "- elsewhere memory\n");
        t.put(&t.mem_path(&cwd), "- index line\n");
        t.put(
            &t.mem_path(&t.p("top/user/repo/long")),
            &"- line\n".repeat(300),
        );
        t.dir.mkdir("my_app dir/.git");
        let claude_slug: String = t
            .spaced_repo()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        t.put(
            &format!("{}/{claude_slug}/memory/MEMORY.md", t.projects()),
            "- spaced memory\n",
        );
        t.put(
            &format!("{}/{}/memory/topic.md", t.projects(), encode_slug(&cwd)),
            &"y".repeat(8000),
        );
        t
    }

    fn gather(&self, at: &str) -> Vec<OnDiskContextFile> {
        gather_on_disk_context_files(Some(at), &self.home(), &self.projects())
    }

    fn paths(&self, at: &str) -> Vec<String> {
        self.gather(at).into_iter().map(|f| f.path).collect()
    }

    fn memory(&self, at: &str) -> Vec<OnDiskContextFile> {
        self.gather(at)
            .into_iter()
            .filter(|f| f.scope == ContextScope::Memory)
            .collect()
    }
}

#[cfg(unix)]
#[test]
fn reads_parent_dir_claude_md_and_local() {
    let t = Tree::new();
    let paths = t.paths(&t.cwd());
    for want in [
        format!("{}/CLAUDE.md", t.repo()),
        format!("{}/CLAUDE.local.md", t.repo()),
        format!("{}/CLAUDE.local.md", t.cwd()),
        format!("{}/CLAUDE.md", t.cwd()),
        format!("{}/CLAUDE.md", t.home()),
    ] {
        assert!(paths.contains(&want), "{want}");
    }
}

#[cfg(unix)]
#[test]
fn skips_agents_md_when_the_project_has_claude_md() {
    let t = Tree::new();
    assert!(
        !t.paths(&t.cwd())
            .contains(&format!("{}/AGENTS.md", t.cwd()))
    );
}

#[cfg(unix)]
#[test]
fn reads_agents_md_when_the_project_has_no_claude_file() {
    let t = Tree::new();
    let paths = t.paths(&t.p("agents-only/app"));
    assert!(paths.contains(&t.p("agents-only/AGENTS.md")));
    assert!(paths.contains(&t.p("agents-only/app/.claude/AGENTS.md")));
}

#[cfg(unix)]
#[test]
fn counts_only_memory_index() {
    let t = Tree::new();
    let mem = t.memory(&t.cwd());
    assert_eq!(mem.len(), 1);
    assert_eq!(mem[0].bytes, units_len("- index line"));
}

#[cfg(unix)]
#[test]
fn walks_above_the_users_home() {
    let t = Tree::new();
    assert!(t.paths(&t.cwd()).contains(&t.p("top/CLAUDE.local.md")));
}

#[cfg(unix)]
#[test]
fn lists_ancestors_farthest_first_after_the_global_file() {
    let t = Tree::new();
    let base = t.base();
    let paths: Vec<_> = t
        .paths(&t.cwd())
        .into_iter()
        .filter(|p| p.starts_with(&base))
        .collect();
    let (home, repo, cwd) = (t.home(), t.repo(), t.cwd());
    assert_eq!(
        paths,
        [
            format!("{home}/CLAUDE.md"),
            format!("{home}/rules/style.md"),
            t.p("top/CLAUDE.local.md"),
            format!("{repo}/CLAUDE.md"),
            format!("{repo}/.claude/rules/a/nested.md"),
            format!("{repo}/.claude/rules/b.md"),
            format!("{repo}/.claude/rules/everywhere.md"),
            format!("{repo}/CLAUDE.local.md"),
            format!("{cwd}/CLAUDE.md"),
            format!("{cwd}/CLAUDE.local.md"),
            t.mem_path(&cwd),
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_nested_worktree_skips_the_main_checkouts_project_files() {
    let t = Tree::new();
    let paths = t.paths(&format!("{}/src", t.worktree()));
    let main = t.main_repo();
    assert!(paths.contains(&format!("{}/CLAUDE.md", t.worktree())));
    assert!(paths.contains(&format!("{main}/CLAUDE.local.md")));
    assert!(!paths.contains(&format!("{main}/CLAUDE.md")));
    assert!(!paths.contains(&format!("{main}/.trees/CLAUDE.md")));
    assert!(!paths.contains(&format!("{main}/.claude/rules/main.md")));
}

#[cfg(unix)]
#[test]
fn reads_unconditional_rules_and_skips_scoped_ones() {
    let t = Tree::new();
    let files = t.gather(&t.cwd());
    let paths: Vec<_> = files.iter().map(|f| f.path.clone()).collect();
    assert!(!paths.contains(&format!("{}/rules/scoped.md", t.home())));
    assert!(!paths.contains(&format!("{}/.claude/rules/scoped.md", t.repo())));
    assert!(!paths.contains(&format!("{}/.claude/rules/notes.txt", t.repo())));
    let user = files
        .iter()
        .find(|f| f.path.ends_with("style.md"))
        .expect("user rule");
    assert_eq!(user.scope, ContextScope::Global);
}

#[cfg(unix)]
#[test]
fn keys_memory_by_the_main_checkout_from_a_nested_worktree() {
    let t = Tree::new();
    let mem: Vec<_> = t
        .memory(&format!("{}/src", t.worktree()))
        .into_iter()
        .map(|f| f.path)
        .collect();
    assert_eq!(mem, [t.mem_path(&t.main_repo())]);
}

#[cfg(unix)]
#[test]
fn keys_memory_by_the_main_checkout_from_a_subdir() {
    let t = Tree::new();
    let mem = t.memory(&format!("{}/src", t.main_repo()));
    assert_eq!(mem[0].bytes, units_len("- main memory"));
}

#[cfg(unix)]
#[test]
fn keys_memory_by_the_main_checkout_from_an_outside_worktree() {
    let t = Tree::new();
    let mem = t.memory(&t.outside_tree());
    assert_eq!(mem[0].bytes, units_len("- elsewhere memory"));
}

#[cfg(unix)]
#[test]
fn the_main_checkout_reads_its_claude_md() {
    let t = Tree::new();
    let main = t.main_repo();
    assert!(t.paths(&main).contains(&format!("{main}/CLAUDE.md")));
}

#[cfg(unix)]
#[test]
fn keys_memory_by_claudes_slug_for_a_spaced_cwd() {
    let t = Tree::new();
    let mem = t.memory(&format!("{}/src", t.spaced_repo()));
    assert_eq!(mem[0].bytes, units_len("- spaced memory"));
}

#[cfg(unix)]
#[test]
fn caps_memory_at_200_lines() {
    let t = Tree::new();
    let mem = t.memory(&t.p("top/user/repo/long"));
    assert_eq!(mem[0].bytes, units_len(&"- line\n".repeat(200)) - 1.0);
}

#[test]
fn cap_memory_counts_characters_not_bytes() {
    let text = format!("{}\n", "я".repeat(150)).repeat(150);
    assert_eq!(cap_memory(&text), text.trim());
}

#[test]
fn cap_memory_cuts_past_the_char_limit_at_the_last_newline() {
    let line = "x".repeat(99);
    let lines = format!("{line}\n").repeat(190);
    let out = cap_memory(&format!("{lines}{}", "y".repeat(10_000)));
    assert_eq!(out, lines.trim_end());
}

#[test]
fn cap_memory_cuts_at_the_limit_without_a_newline() {
    assert_eq!(cap_memory(&"z".repeat(30_000)).len(), 25_000);
}

#[test]
fn rule_globs_reads_inline_bracketed_and_listed_paths() {
    assert_eq!(
        rule_globs("---\npaths: a/**, b/*.ts\n---\n"),
        ["a", "b/*.ts"]
    );
    assert_eq!(
        rule_globs("---\npaths: [\"src/{a,b}.ts\", lib]\n---\n"),
        ["src/{a,b}.ts", "lib"]
    );
    assert_eq!(
        rule_globs("---\npaths:\n  - x/**\n  - 'y'\nname: n\n---\n"),
        ["x", "y"]
    );
}

#[test]
fn rule_globs_is_empty_without_frontmatter_or_paths() {
    assert_eq!(rule_globs("paths: a\n"), Vec::<String>::new());
    assert_eq!(
        rule_globs("---\nname: n\n---\nbody\n"),
        Vec::<String>::new()
    );
}

// resolve-context

const ID_A: &str = "aaaaaaaa-1111-4aaa-8aaa-aaaaaaaaaaaa";
const ID_B: &str = "aaaaaaaa-2222-4aaa-8aaa-aaaaaaaaaaaa";
const ID_C: &str = "cccccccc-3333-4ccc-8ccc-cccccccccccc";

/// `<base>/claude` is the config dir, `<base>/work/demo` the cwd.
fn resolve_tree() -> (TempDir, String) {
    let dir = TempDir::new("resolve");
    let base = dir.str();
    let cwd = format!("{base}/work/demo");
    dir.write("claude/CLAUDE.md", &format!("{:.<40}", "global rules"));
    dir.write("work/demo/AGENTS.md", "project rules");
    dir.write("work/demo/.claude/CLAUDE.md", "nested");
    let slug = encode_slug(&cwd);
    dir.write(&format!("claude/projects/{slug}/memory/MEMORY.md"), "1234");
    dir.write(
        &format!("claude/projects/{slug}/memory/sub/note.md"),
        "12345678",
    );
    for id in [ID_A, ID_B, ID_C] {
        dir.write(&format!("claude/projects/{slug}/{id}.jsonl"), "{}\n");
    }
    let subs = format!("claude/projects/{slug}/{ID_C}/subagents");
    dir.write(&format!("{subs}/agent-x1.jsonl"), "{}\n");
    dir.write(
        &format!("{subs}/agent-x1.meta.json"),
        r#"{"agentType":"Explore","description":"scan files"}"#,
    );
    (dir, cwd)
}

#[cfg(unix)]
#[test]
fn collects_global_project_and_memory_files() {
    let (dir, cwd) = resolve_tree();
    let base = dir.str();
    let all = gather_on_disk_context_files(
        Some(&cwd),
        &format!("{base}/claude"),
        &format!("{base}/claude/projects"),
    );
    let files: Vec<_> = all.iter().filter(|f| f.path.starts_with(&base)).collect();
    let got: Vec<_> = files.iter().map(|f| (f.scope, f.bytes)).collect();
    assert_eq!(
        got,
        [
            (ContextScope::Global, 40.0),
            (ContextScope::Project, 6.0),
            (ContextScope::Memory, 4.0)
        ]
    );
    assert_eq!(files[2].label, "memory/MEMORY.md");
}

#[test]
fn unique_prefix_resolves_and_ambiguous_is_not_found() {
    let (dir, _) = resolve_tree();
    let base = dir.str();
    let claude = sources(&format!("{base}/home"), &format!("{base}/claude/projects"));
    let hit = resolve_claude_session(&claude, "cccccccc").expect("hit");
    assert_eq!(hit.session_id, ID_C);
    assert!(
        hit.subagent_dir
            .as_deref()
            .is_some_and(|d| d.ends_with("/subagents"))
    );
    let err = resolve_claude_session(&claude, "aaaaaaaa");
    assert!(matches!(err, Err(SessionError::NotFound { .. })), "{err:?}");
}

#[test]
fn find_subagents_reads_meta_and_no_dir_yields_none() {
    let (dir, _) = resolve_tree();
    let base = dir.str();
    let claude = sources(&format!("{base}/home"), &format!("{base}/claude/projects"));
    let loc = resolve_claude_session(&claude, ID_C).expect("hit");
    let stubs = find_subagents(loc.subagent_dir.as_deref());
    let got: Vec<_> = stubs
        .iter()
        .map(|s| (s.agent_type.as_deref(), s.description.as_deref()))
        .collect();
    assert_eq!(got, [(Some("Explore"), Some("scan files"))]);
    assert_eq!(find_subagents(None), Vec::new());
}
