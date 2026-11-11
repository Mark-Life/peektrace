//! Rule present but still firing: memory files read from disk, symlinks
//! followed and deduped, then matched against each cause's probes.
#![allow(clippy::float_cmp, reason = "dates and rates are exact")]

mod common;

use std::cell::RefCell;

use common::{TempDir, set_mtime};
use peektrace_core::Agent;
use peektrace_core::js::time::{Utc, parse_date};
use peektrace_core::roots::Source;
use peektrace_core::stats::detectors::causes::GIT_DIFF_FIX;
use peektrace_core::stats::detectors::rules::{
    CauseCounts, CodexCover, CoverSince, MemoryDoc, RowRef, RuleCheck, RuleCover, Split, check_row,
};
use peektrace_core::stats::memory_files::{AgentHomes, read_memory_docs};
use peektrace_core::stats::redact::Redactor;
use peektrace_core::stats::rule_dates::{LineDates, LineLedger, date_lines};
use peektrace_core::stats::schema::CauseId;

const SECRET: &str = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";

fn ms(iso: &str) -> f64 {
    #[allow(clippy::cast_precision_loss, reason = "epoch ms fit in f64")]
    let v = parse_date(iso, &Utc).expect("date") as f64;
    v
}

/// The TS `beforeAll` layout under `<base>/u`.
fn fixture() -> TempDir {
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
    std::os::unix::fs::symlink(&shared, t.path("u/.claude/CLAUDE.md")).expect("symlink");
    std::os::unix::fs::symlink(&shared, t.path("u/.codex/AGENTS.md")).expect("symlink");
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

fn source(id: &str, home: String, projects: String) -> Source {
    Source {
        id: id.to_owned(),
        label: id.to_owned(),
        home,
        projects_root: projects,
    }
}

fn read_with(t: &TempDir, claude: &[Source], dirs: &[String], scope: &[Agent]) -> Vec<MemoryDoc> {
    let homes = AgentHomes {
        claude,
        codex_home: t.path("u/.codex"),
        pi_home: t.path("u/.pi"),
    };
    read_memory_docs(&homes, scope, dirs)
}

fn read_docs(t: &TempDir, dirs: &[String], claude_dir: &str, scope: &[Agent]) -> Vec<MemoryDoc> {
    let claude = [source(
        "claude",
        t.path(&format!("u/{claude_dir}")),
        t.path("u/projects"),
    )];
    read_with(t, &claude, dirs, scope)
}

const CLAUDE_CODEX: [Agent; 2] = [Agent::Claude, Agent::Codex];

fn cover(agent: Agent, dir: Option<&str>) -> RuleCover {
    RuleCover {
        agent,
        codex: None,
        dir: dir.map(str::to_owned),
        source: None,
    }
}

fn codex_cover(dir: &str, exact: bool, stops: &[String]) -> RuleCover {
    RuleCover {
        codex: Some(CodexCover {
            exact,
            stops: stops.to_vec(),
        }),
        ..cover(Agent::Codex, Some(dir))
    }
}

#[test]
fn follows_symlinks_dedupes_and_collapses_home() {
    let t = fixture();
    let docs = read_docs(&t, &[t.path("u/proj")], ".claude", &CLAUDE_CODEX);
    let shape: Vec<(Agent, bool)> = docs
        .iter()
        .map(|d| (d.cover.agent, d.cover.dir.is_some()))
        .collect();
    assert_eq!(
        shape,
        [
            (Agent::Claude, false),
            (Agent::Claude, false),
            (Agent::Codex, false),
            (Agent::Codex, false),
            (Agent::Codex, true),
        ]
    );
    assert_eq!(
        docs[2].display_path,
        "~/.codex/AGENTS.md → ~/.agents/AGENTS.md"
    );
    assert_eq!(docs[2].cover, cover(Agent::Codex, None));
    assert_eq!(
        docs[0].display_path,
        "~/.claude/CLAUDE.md → ~/.agents/AGENTS.md"
    );
    assert_eq!(docs[0].cover, cover(Agent::Claude, None));
    assert_eq!(docs[0].mtime_ms, Some(ms("2026-08-01T12:00:00.000Z")));
    assert_eq!(docs[4].display_path, "~/proj/AGENTS.md");
    assert_eq!(docs[4].cover, codex_cover(&t.path("u/proj"), false, &[]));
}

#[test]
fn claude_global_and_project_rules_dirs_are_read() {
    let t = fixture();
    t.write("u/.claude/rules/zsh.md", "- Quote every glob.");
    t.write("u/.claude/rules/notes.txt", "- Not a rule file.");
    t.write("u/proj/.claude/rules/shell/pkill.md", "- pkill -x only.");
    let docs = read_docs(&t, &[t.path("u/proj/app")], ".claude", &[Agent::Claude]);
    let rules: Vec<(&str, &RuleCover)> = docs
        .iter()
        .filter(|d| d.display_path.contains("/rules/"))
        .map(|d| (d.display_path.as_str(), &d.cover))
        .collect();
    assert_eq!(
        rules,
        [
            ("~/.claude/rules/zsh.md", &cover(Agent::Claude, None)),
            (
                "~/proj/.claude/rules/shell/pkill.md",
                &cover(Agent::Claude, Some(&t.path("u/proj")))
            ),
        ]
    );
}

#[test]
fn codex_walks_up_only_to_the_repo_root() {
    let t = fixture();
    t.mkdir("u/work/repo/src/.git");
    t.mkdir("u/work/loose/sub");
    t.write("u/work/AGENTS.md", "- Above every repo.");
    t.write("u/work/loose/AGENTS.md", "- Above the cwd.");
    let in_repo = t.path("u/work/repo/src");
    let loose = t.path("u/work/loose/sub");
    let docs = read_docs(&t, &[in_repo.clone(), loose], ".claude", &[Agent::Codex]);
    assert!(docs.iter().all(|d| d.cover.dir.is_none()));
    let pi = read_docs(&t, &[in_repo], ".claude", &[Agent::Pi]);
    assert!(pi.iter().any(|d| d.display_path == "~/work/AGENTS.md"));
}

#[test]
fn a_codex_file_reaches_its_repo_but_not_a_nested_one() {
    let t = fixture();
    t.mkdir("u/nest/outer/.git");
    let inner = t.mkdir("u/nest/outer/vendor/inner");
    t.mkdir("u/nest/outer/vendor/inner/.git");
    t.write("u/nest/outer/AGENTS.md", "- Outer rules.");
    t.write("u/nest/loose/AGENTS.md", "- Loose rules.");
    let docs = read_docs(
        &t,
        &[
            t.path("u/nest/outer"),
            inner.clone(),
            t.path("u/nest/loose"),
        ],
        ".claude",
        &[Agent::Codex],
    );
    let covers: Vec<&RuleCover> = docs
        .iter()
        .filter(|d| d.cover.dir.is_some())
        .map(|d| &d.cover)
        .collect();
    assert_eq!(
        covers,
        [
            &codex_cover(&t.path("u/nest/loose"), true, &[]),
            &codex_cover(&t.path("u/nest/outer"), false, &[inner]),
        ]
    );
}

#[test]
fn each_claude_sources_global_file_reaches_its_own_sessions() {
    let t = fixture();
    t.write("u/.claude-work/CLAUDE.md", "- Work rules.");
    let claude = [
        source("claude", t.path("u/.claude"), t.path("u/projects")),
        source(
            "work",
            t.path("u/.claude-work"),
            t.path("u/.claude-work/projects"),
        ),
    ];
    let docs = read_with(&t, &claude, &[], &[Agent::Claude]);
    let got: Vec<(&str, Option<&str>)> = docs
        .iter()
        .map(|d| {
            (
                d.display_path.as_str(),
                d.cover.source.as_ref().map(|s| s.0.as_str()),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("~/.claude/CLAUDE.md → ~/.agents/AGENTS.md", Some("claude")),
            ("~/.agents/skills/rm.md", Some("claude")),
            ("~/.claude-work/CLAUDE.md", Some("work")),
        ]
    );
}

#[test]
fn a_repo_outside_the_home_keeps_its_root_file() {
    let t = fixture();
    let repo = t.mkdir("outside/repo");
    let pkg = t.mkdir("outside/repo/packages/app");
    t.mkdir("outside/repo/.git");
    t.write("outside/repo/CLAUDE.md", "- Repo-root Claude rules.");
    t.write("outside/repo/AGENTS.md", "- Repo-root Codex rules.");
    let docs = read_docs(&t, &[pkg], ".claude", &CLAUDE_CODEX);
    let own: Vec<(Agent, Option<&str>)> = docs
        .iter()
        .filter(|d| d.real_path.starts_with(&repo))
        .map(|d| (d.cover.agent, d.cover.dir.as_deref()))
        .collect();
    assert!(own.contains(&(Agent::Claude, Some(repo.as_str()))));
    assert!(own.contains(&(Agent::Codex, Some(repo.as_str()))));
}

#[test]
fn opencode_reads_its_global_file_else_claudes() {
    let t = fixture();
    let fallback = read_docs(&t, &[], ".claude", &[Agent::Opencode]);
    let got: Vec<(&str, &RuleCover)> = fallback
        .iter()
        .map(|d| (d.display_path.as_str(), &d.cover))
        .collect();
    assert_eq!(
        got,
        [
            (
                "~/.claude/CLAUDE.md → ~/.agents/AGENTS.md",
                &cover(Agent::Opencode, None)
            ),
            ("~/.agents/skills/rm.md", &cover(Agent::Opencode, None)),
        ]
    );
    t.write("u/.config/opencode/AGENTS.md", "- OpenCode global rules.");
    let own = read_docs(&t, &[], ".claude", &[Agent::Opencode]);
    let paths: Vec<&str> = own.iter().map(|d| d.display_path.as_str()).collect();
    assert_eq!(paths, ["~/.config/opencode/AGENTS.md"]);
}

#[test]
fn each_agent_reads_only_its_own_project_files() {
    let t = fixture();
    let docs = read_docs(
        &t,
        &[t.path("u/proj/app")],
        ".claude",
        &[Agent::Claude, Agent::Codex, Agent::Pi],
    );
    let shown: Vec<(&str, Agent)> = docs
        .iter()
        .filter(|d| d.cover.dir.is_some())
        .map(|d| (d.display_path.as_str(), d.cover.agent))
        .collect();
    assert_eq!(
        shown,
        [
            ("~/proj/AGENTS.md", Agent::Codex),
            ("~/proj/AGENTS.md", Agent::Pi),
            ("~/proj/app/CLAUDE.local.md", Agent::Claude),
            ("~/proj/app/AGENTS.override.md", Agent::Codex),
            ("~/proj/app/AGENTS.override.md", Agent::Pi),
        ]
    );
}

#[test]
fn pis_global_context_file_is_read() {
    let t = fixture();
    let docs = read_docs(&t, &[], ".claude", &[Agent::Pi]);
    let got: Vec<(&str, &RuleCover)> = docs
        .iter()
        .map(|d| (d.display_path.as_str(), &d.cover))
        .collect();
    assert_eq!(got, [("~/.pi/agent/AGENTS.md", &cover(Agent::Pi, None))]);
}

#[test]
fn an_import_keeps_its_importers_reach_a_code_span_is_not_one() {
    let t = fixture();
    let docs = read_docs(&t, &[], ".claude", &CLAUDE_CODEX);
    assert_eq!(docs[1].display_path, "~/.agents/skills/rm.md");
    assert_eq!(docs[1].cover, cover(Agent::Claude, None));
    assert!(!docs.iter().any(|d| d.display_path.contains("missing")));
}

#[test]
fn ancestors_and_claude_local_of_a_project_dir_are_read() {
    let t = fixture();
    let docs = read_docs(&t, &[t.path("u/proj/app")], ".claude", &CLAUDE_CODEX);
    let shown: Vec<(&str, Option<&str>)> = docs
        .iter()
        .map(|d| (d.display_path.as_str(), d.cover.dir.as_deref()))
        .collect();
    let app = t.path("u/proj/app");
    let proj = t.path("u/proj");
    assert!(shown.contains(&("~/proj/app/CLAUDE.local.md", Some(app.as_str()))));
    assert!(shown.contains(&("~/proj/AGENTS.md", Some(proj.as_str()))));
}

#[test]
fn a_moved_claude_home_still_finds_the_users_home() {
    let t = fixture();
    let docs = read_docs(&t, &[t.path("u/proj/app")], ".claude-work", &CLAUDE_CODEX);
    let shown: Vec<&str> = docs.iter().map(|d| d.display_path.as_str()).collect();
    assert!(shown.contains(&"~/proj/AGENTS.md"));
    assert!(shown.contains(&"~/.agents/skills/rm.md"));
}

type Asked = RefCell<Vec<Vec<(RuleCover, String)>>>;

/// Counts that reach every file and remember the covers the check asked about.
struct Recording<'a>(&'a Asked);

impl CauseCounts for Recording<'_> {
    fn since(&self, _key: &str, covers: &[CoverSince<'_>]) -> (Split, Split) {
        self.0.borrow_mut().push(
            covers
                .iter()
                .map(|c| (c.cover.clone(), c.after_day.clone()))
                .collect(),
        );
        (
            Split {
                main: 1.0,
                nested: 9.0,
            },
            Split {
                main: 100.0,
                nested: 400.0,
            },
        )
    }
}

/// Only Claude made calls: a Codex-only file reaches no hit.
struct ClaudeOnly;

impl CauseCounts for ClaudeOnly {
    fn since(&self, _key: &str, covers: &[CoverSince<'_>]) -> (Split, Split) {
        let n = if covers.iter().any(|c| c.cover.agent != Agent::Codex) {
            1.0
        } else {
            0.0
        };
        (
            Split {
                main: n,
                nested: 0.0,
            },
            Split {
                main: 100.0 * n,
                nested: 0.0,
            },
        )
    }
}

/// A file changed on the last day: hits only when no day bound is asked.
struct Fresh;

impl CauseCounts for Fresh {
    fn since(&self, _key: &str, covers: &[CoverSince<'_>]) -> (Split, Split) {
        if covers.iter().all(|c| c.after_day.is_empty()) {
            (
                Split {
                    main: 1.0,
                    nested: 0.0,
                },
                Split {
                    main: 10.0,
                    nested: 0.0,
                },
            )
        } else {
            (Split::default(), Split::default())
        }
    }
}

fn row(cause: CauseId) -> RowRef<'static> {
    RowRef {
        key: Box::leak(format!("cause:{}", cause.as_str()).into_boxed_str()),
        cause,
        fix: Some("quote"),
        details: &[],
    }
}

fn check(row: &RowRef<'_>, docs: &[MemoryDoc], counts: &dyn CauseCounts) -> Option<RuleCheck> {
    check_row(
        row,
        docs,
        counts,
        &LineDates::by_mtime(),
        &Redactor::new(true),
    )
}

fn any_counts() -> Recording<'static> {
    Recording(Box::leak(Box::default()))
}

fn one_doc(lines: &[&str]) -> Vec<MemoryDoc> {
    vec![MemoryDoc {
        display_path: "~/x.md".into(),
        real_path: "/x.md".into(),
        cover: cover(Agent::Claude, None),
        mtime_ms: None,
        lines: lines.iter().map(|l| (*l).to_owned()).collect(),
    }]
}

fn app_docs(t: &TempDir) -> Vec<MemoryDoc> {
    read_docs(t, &[t.path("u/proj/app")], ".claude", &CLAUDE_CODEX)
}

#[test]
fn a_probe_matching_one_line_wins_over_a_window() {
    let t = fixture();
    let asked = Asked::default();
    let rule = check(
        &row(CauseId::ZshGlobFlag),
        &app_docs(&t),
        &Recording(&asked),
    )
    .expect("rule");
    let lines: Vec<f64> = rule.matches.iter().map(|m| m.line).collect();
    assert_eq!(lines, [4.0]);
    assert_eq!(rule.since_iso.as_deref(), Some("2026-08-01T12:00:00.000Z"));
    assert_eq!(
        asked.borrow().last().cloned().unwrap_or_default(),
        [
            (cover(Agent::Claude, None), "2026-08-01".to_owned()),
            (cover(Agent::Codex, None), "2026-08-01".to_owned()),
        ]
    );
    assert_eq!(
        rule.per1k_since,
        Split {
            main: 10.0,
            nested: 22.5
        }
    );
    assert!(rule.advice.contains("setopt nonomatch"));
    assert!(rule.advice.contains("90% of hits come from subagents"));
}

#[test]
fn a_file_changed_on_the_last_day_says_what_to_do() {
    let t = fixture();
    let rule = check(&row(CauseId::ZshGlobFlag), &app_docs(&t), &Fresh).expect("rule");
    assert!(rule.advice.starts_with("if it keeps firing: "));
    assert!(rule.advice.contains("setopt nonomatch"));
}

#[test]
fn a_file_that_reaches_none_of_the_hits_does_not_count() {
    let docs = vec![MemoryDoc {
        cover: cover(Agent::Codex, None),
        display_path: "~/.codex/AGENTS.md".into(),
        real_path: "/c/AGENTS.md".into(),
        ..one_doc(&["- Quote every glob in a flag value."]).remove(0)
    }];
    assert!(check(&row(CauseId::ZshGlobFlag), &docs, &ClaudeOnly).is_none());
}

#[test]
fn flag_glob_line_is_not_the_path_rule_node_modules_is_not_a_module_rule() {
    let t = fixture();
    let docs = app_docs(&t);
    assert!(check(&row(CauseId::ZshGlobPath), &docs, &any_counts()).is_none());
    assert!(check(&row(CauseId::ScratchModuleResolution), &docs, &any_counts()).is_none());
}

#[test]
fn a_cause_with_no_matching_line_or_probe_has_no_rule() {
    let t = fixture();
    let docs = app_docs(&t);
    assert!(check(&row(CauseId::UgrepComplexity), &docs, &any_counts()).is_none());
    let schema = RowRef {
        key: "cause:x",
        ..row(CauseId::StructuredOutputSchema)
    };
    assert!(check(&schema, &docs, &any_counts()).is_none());
}

fn cnf_row(details: &'static [(String, f64)]) -> RowRef<'static> {
    RowRef {
        details,
        ..row(CauseId::CommandNotFound)
    }
}

fn details(bins: &[(&str, f64)]) -> &'static [(String, f64)] {
    Box::leak(
        bins.iter()
            .map(|(b, n)| ((*b).to_owned(), *n))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

#[test]
fn a_missing_binary_matches_only_a_line_saying_it_is_missing() {
    let t = fixture();
    let r = cnf_row(details(&[("jq", 3.0), ("file", 1.0)]));
    let rule = check(&r, &app_docs(&t), &any_counts()).expect("rule");
    let lines: Vec<f64> = rule.matches.iter().map(|m| m.line).collect();
    assert_eq!(lines, [2.0]);
}

#[test]
fn prose_saying_no_file_was_found_is_not_a_missing_binary_rule() {
    let r = cnf_row(details(&[("file", 1.0)]));
    let has = |line: &str| check(&r, &one_doc(&[line]), &any_counts()).is_some();
    assert!(!has("- If the search finds no file, say so."));
    assert!(has("- This box has no file: use stat."));
    assert!(has("- There is no `file` binary here."));
}

#[test]
fn a_binarys_name_must_be_a_whole_word() {
    let has = |bin: &str, line: &str| {
        check(
            &cnf_row(details(&[(bin, 1.0)])),
            &one_doc(&[line]),
            &any_counts(),
        )
        .is_some()
    };
    assert!(!has("file", "- Never install files with sudo; ask first."));
    assert!(!has(
        "file",
        "- Edit in place without filesystem-wide searches."
    ));
    assert!(!has(
        "node",
        "- Use bun instead of node_modules/.bin scripts."
    ));
    assert!(!has(
        "jq",
        "- Prefer python instead of jquery-style parsing."
    ));
    assert!(!has("bc", "- Do not install bcrypt globally."));
    assert!(!has("file", "- Never install file.txt fixtures."));
    assert!(has("jq", "- Use bun -e instead of jq."));
    assert!(has("rsync", "- Install `rsync` before syncing."));
}

#[test]
fn a_loose_line_is_not_a_rule() {
    let has = |r: &RowRef<'_>, line: &str| check(r, &one_doc(&[line]), &any_counts()).is_some();
    let equals = row(CauseId::ZshEqualsAbort);
    let output = row(CauseId::OutputTooLarge);
    let path = row(CauseId::ZshGlobPath);
    assert!(!has(&equals, "- Use `===` and `!==`, never `==`."));
    assert!(has(&equals, "- Quote separator echoes: `echo \"===\"`."));
    assert!(!has(
        &output,
        "- Before committing, review with git diff HEAD."
    ));
    assert!(!has(
        &path,
        "- Use the Glob tool instead of find for file search."
    ));
    assert!(has(
        &path,
        "- Not `ls dir/*.x` or `(N)` globs: zsh aborts on them."
    ));
    let diff_line = "- Run `git diff --stat` first, then one file at a time.";
    assert!(!has(&output, diff_line));
    let git = RowRef {
        fix: Some(GIT_DIFF_FIX),
        ..row(CauseId::OutputTooLarge)
    };
    assert!(has(&git, diff_line));
    assert!(has(
        &output,
        "- Bound shell output: pipe it through | head -n 200."
    ));
}

#[test]
fn every_file_counts_from_the_newest_ones_day() {
    let doc = |path: &str, day: &str, dir: Option<&str>| MemoryDoc {
        display_path: path.into(),
        real_path: path.into(),
        cover: cover(Agent::Claude, dir),
        mtime_ms: Some(ms(&format!("{day}T12:00:00Z"))),
        lines: vec!["Quote every glob passed as a flag value.".into()],
    };
    let asked = Asked::default();
    let rule = check(
        &row(CauseId::ZshGlobFlag),
        &[
            doc("~/.claude/CLAUDE.md", "2026-10-01", None),
            doc("~/proj/CLAUDE.md", "2026-10-21", Some("/u/proj")),
        ],
        &Recording(&asked),
    )
    .expect("rule");
    assert_eq!(
        rule.since_iso.as_deref().map(|s| &s[..10]),
        Some("2026-10-21")
    );
    assert_eq!(rule.matches[0].path, "~/proj/CLAUDE.md");
    let days: Vec<String> = asked
        .borrow()
        .last()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|c| c.1)
        .collect();
    assert_eq!(days, ["2026-10-21", "2026-10-21"]);
}

#[test]
fn a_secret_on_a_matched_line_is_redacted() {
    let line = format!("Quote every glob; token {SECRET}");
    let rule = check(
        &row(CauseId::ZshGlobFlag),
        &one_doc(&[&line]),
        &any_counts(),
    )
    .expect("rule");
    assert!(!rule.matches[0].excerpt.contains(SECRET));
    assert_eq!(rule.since_iso, None);
}

#[test]
fn a_secret_straddling_the_clip_is_redacted_whole() {
    let lead = format!("Quote every glob; {} ", "x".repeat(160 - 18 - 1 - 16));
    let line = format!("{lead}{SECRET} end");
    let rule = check(
        &row(CauseId::ZshGlobFlag),
        &one_doc(&[&line]),
        &any_counts(),
    )
    .expect("rule");
    let excerpt = &rule.matches[0].excerpt;
    assert!(!excerpt.contains("sk-ant-"));
    assert!(excerpt.contains("[REDACTED:"));
}

const GLOB_LINE: &str = "- Quote every glob passed as a flag value.";

fn dated_doc(lines: &[&str], day: &str) -> MemoryDoc {
    MemoryDoc {
        display_path: "~/.agents/AGENTS.md".into(),
        real_path: "/u/.agents/AGENTS.md".into(),
        cover: cover(Agent::Claude, None),
        mtime_ms: Some(ms(&format!("{day}T12:00:00Z"))),
        lines: lines.iter().map(|l| (*l).to_owned()).collect(),
    }
}

#[test]
fn editing_another_line_keeps_a_rules_date() {
    let now = ms("2026-10-21T18:00:00Z");
    let (_, first) = date_lines(
        &[dated_doc(&[GLOB_LINE], "2026-10-15")],
        &LineLedger::default(),
        now,
    );
    let edited = dated_doc(&["- A new line.", GLOB_LINE], "2026-10-21");
    let (dates, _) = date_lines(std::slice::from_ref(&edited), &first, now);
    assert_eq!(dates.date_of(&edited, 1), Some(ms("2026-10-15T12:00:00Z")));
    assert_eq!(dates.date_of(&edited, 0), Some(ms("2026-10-21T12:00:00Z")));
    let asked = Asked::default();
    let rule = check_row(
        &row(CauseId::ZshGlobFlag),
        std::slice::from_ref(&edited),
        &Recording(&asked),
        &dates,
        &Redactor::new(true),
    )
    .expect("rule");
    assert_eq!(
        rule.since_iso.as_deref().map(|s| &s[..10]),
        Some("2026-10-15")
    );
    let days: Vec<String> = asked
        .borrow()
        .last()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|c| c.1)
        .collect();
    assert_eq!(days, ["2026-10-15"]);
}

#[test]
fn the_ledger_stores_hashes_and_drops_gone_lines() {
    let now = ms("2026-10-21T18:00:00Z");
    let secret_line = format!("token {SECRET}");
    let (_, first) = date_lines(
        &[dated_doc(&[GLOB_LINE, &secret_line], "2026-10-15")],
        &LineLedger::default(),
        now,
    );
    assert!(
        first
            .files
            .iter()
            .all(|(p, keys)| !p.contains("sk-ant") && keys.keys().all(|k| !k.contains("sk-ant")))
    );
    let (_, second) = date_lines(&[dated_doc(&[GLOB_LINE], "2026-10-21")], &first, now);
    assert_eq!(
        second
            .files
            .get("/u/.agents/AGENTS.md")
            .map(indexmap::IndexMap::len),
        Some(1)
    );
}
