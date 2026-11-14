//! Cases from the inspector's `memory-support.test.ts`,
//! `memory-project-options.test.ts` and the browse table's rules.

use peektrace_js::time::Utc;
use peektrace_protocol::view::memory::{EntryView, LinkView};
use peektrace_protocol::{CapabilitySupport, FileChangeReason, PerAgentSupport};
use serde_json::json;

use super::*;

fn cell(level: SupportLevel, note: Option<&str>) -> CapabilitySupport {
    CapabilitySupport {
        level,
        note: note.map(str::to_owned),
    }
}

fn crud(levels: [SupportLevel; 4], note: Option<&str>) -> Vec<Capability> {
    let [claude, codex, pi, opencode] = levels;
    vec![Capability {
        id: MEMORY_CRUD.to_owned(),
        group: "memory".to_owned(),
        title: "Memory CRUD".to_owned(),
        description: String::new(),
        per_agent: PerAgentSupport {
            claude: cell(claude, note),
            codex: cell(codex, None),
            pi: cell(pi, None),
            opencode: cell(opencode, None),
        },
    }]
}

use SupportLevel::{Partial, Planned, Supported, Unsupported};

#[test]
fn summary_names_supported_agents_and_groups_the_rest() {
    let s = |l| support_summary(&crud_cells(&crud(l, None)));
    assert_eq!(
        s([Supported, Planned, Planned, Unsupported]),
        "Memory editing: Claude. Planned: Codex, Pi. Not supported: OpenCode."
    );
    assert_eq!(
        s([Supported, Supported, Partial, Planned]),
        "Memory editing: Claude, Codex. Partial: Pi. Planned: OpenCode."
    );
    assert_eq!(
        support_summary(&crud_cells(&[])),
        "Memory editing: no agent yet. Not supported: Claude, Codex, Pi, OpenCode."
    );
}

#[test]
fn verdict_follows_the_claude_cell() {
    let ok = crud_cells(&crud([Supported, Planned, Planned, Planned], None));
    assert_eq!(crud_verdict(&ok, false).blocked_by, None);
    let planned = crud_cells(&crud([Planned, Supported, Planned, Planned], Some("soon")));
    assert_eq!(
        crud_verdict(&planned, false),
        Verdict {
            level: Planned,
            note: Some("soon".to_owned()),
            blocked_by: Some(Blocked::Capability),
        }
    );
    assert_eq!(crud_verdict(&crud_cells(&[]), false).level, Unsupported);
    assert_eq!(crud_verdict(&ok, true).blocked_by, Some(Blocked::ReadOnly));
    assert_eq!(
        capability_title(Unsupported),
        "Memory editing is not supported"
    );
}

const PREFIX: &str = "-home-dev-code-";

fn header(project: &str, updated: Option<&str>, agent: &str) -> SessionHeader {
    serde_json::from_value(json!({
        "id": "x", "agent": agent, "path": "/p", "project": project,
        "messageCount": 0, "sizeBytes": 0, "updatedAt": updated.unwrap_or_default(),
    }))
    .unwrap()
}

fn h(name: &str, updated: Option<&str>) -> SessionHeader {
    header(&format!("{PREFIX}{name}"), updated, "claude")
}

fn label(name: &str, project: &str) -> ProjectLabelView {
    ProjectLabelView {
        slug: format!("{PREFIX}{name}"),
        project: project.to_owned(),
    }
}

fn labels_of(o: &[ProjectOption]) -> Vec<&str> {
    o.iter().map(|o| o.label.as_str()).collect()
}

#[test]
fn project_options_match_the_inspector() {
    assert_eq!(project_options(&[], &[]), vec![]);
    let o = project_options(
        &[
            h("alpha", Some("2026-01-01T00:00:00Z")),
            h("alpha", Some("2026-03-01T00:00:00Z")),
            h("alpha", None),
            h("beta", Some("2026-02-01T00:00:00Z")),
        ],
        &[label("alpha", "alpha"), label("beta", "beta")],
    );
    assert_eq!(labels_of(&o), ["alpha", "beta"]);
    assert_eq!(o[0].last_active.as_deref(), Some("2026-03-01T00:00:00Z"));
    let o = project_options(
        &[
            header("codex-project", Some("2026-05-01T00:00:00Z"), "codex"),
            header("pi-project", Some("2026-05-01T00:00:00Z"), "pi"),
            header("oc-project", Some("2026-05-01T00:00:00Z"), "opencode"),
            h("alpha", Some("2026-01-01T00:00:00Z")),
        ],
        &[],
    );
    assert_eq!(o.len(), 1);
    let o = project_options(
        &[h("alpha", Some("2026-01-01T00:00:00Z"))],
        &[label("alpha", "Alpha App"), label("gamma", "gamma")],
    );
    assert_eq!(
        o,
        vec![
            ProjectOption {
                slug: format!("{PREFIX}alpha"),
                label: "Alpha App".to_owned(),
                last_active: Some("2026-01-01T00:00:00Z".to_owned()),
            },
            ProjectOption {
                slug: format!("{PREFIX}gamma"),
                label: "gamma".to_owned(),
                last_active: None,
            },
        ]
    );
    let o = project_options(
        &[h("zeta", None), h("eta", None)],
        &[
            label("zeta", "zeta"),
            label("eta", "eta"),
            label("theta", "theta"),
        ],
    );
    assert_eq!(labels_of(&o), ["eta", "theta", "zeta"]);
    let o = project_options(&[h("new-app", Some("2026-01-01T00:00:00Z"))], &[]);
    assert_eq!(labels_of(&o), [format!("{PREFIX}new-app")]);
    let o = project_options(
        &[h("web", Some("2026-02-01T00:00:00Z"))],
        &[label("web", "code-web"), label("api", "code-api")],
    );
    assert_eq!(labels_of(&o), ["code-web", "code-api"]);
}

fn entry(slug: &str, body: &str, links: &[(&str, &str)]) -> EntryView {
    EntryView {
        slug: slug.to_owned(),
        file_name: format!("{slug}.md"),
        name: None,
        description: None,
        kind: None,
        body: body.to_owned(),
        size: 1,
        lines: 1,
        mtime: "2026-11-11T16:42:21.851Z".to_owned(),
        mtime_ms: 1.0,
        in_index: false,
        links: links
            .iter()
            .map(|(raw, slug)| LinkView {
                kind: "wiki".to_owned(),
                raw_target: (*raw).to_owned(),
                target_slug: (*slug).to_owned(),
                line: 1,
            })
            .collect(),
    }
}

#[test]
fn equal_keys_sort_in_reverse_input_order_when_descending() {
    let es = [
        entry("a", "", &[]),
        entry("b", "", &[]),
        entry("c", "", &[]),
    ];
    let refs: Vec<&EntryView> = es.iter().collect();
    let slugs = |s: Sort| -> Vec<String> {
        sort_entries(&refs, s)
            .iter()
            .map(|e| e.slug.clone())
            .collect()
    };
    assert_eq!(slugs(Sort::default()), ["c", "b", "a"]);
    assert_eq!(
        slugs(Sort::default().pressed(SortKey::Mtime)),
        ["a", "b", "c"]
    );
    let title = Sort::default().pressed(SortKey::Title);
    assert_eq!(
        title,
        Sort {
            key: SortKey::Title,
            asc: true
        }
    );
    assert!(!title.pressed(SortKey::Title).asc);
}

#[test]
fn sizes_use_1024() {
    let got: Vec<String> = [0, 1023, 1024, 1075, 1536, 1_048_576]
        .into_iter()
        .map(fmt_size)
        .collect();
    assert_eq!(
        got,
        ["0 B", "1023 B", "1.0 KB", "1.0 KB", "1.5 KB", "1024.0 KB"]
    );
    assert_eq!(
        fmt_date("2026-11-11T16:42:21.851Z", &Utc),
        "11/11/2026, 4:42:21 PM"
    );
    assert_eq!(fmt_date("nope", &Utc), "Invalid Date");
}

fn parts(body: &str, links: &[(&str, &str)]) -> Vec<String> {
    body_parts(&entry("x", body, links))
        .into_iter()
        .map(|p| match p {
            BodyPart::Text(t) => format!("t:{t}"),
            BodyPart::Link { text, slug } => format!("l:{text}>{slug}"),
        })
        .collect()
}

#[test]
fn body_parts_split_like_js() {
    assert_eq!(parts("", &[]), ["t:"]);
    assert_eq!(
        parts("[[a]][[B]]", &[]),
        ["t:", "l:[[a]]>a", "t:", "l:[[B]]>b", "t:"]
    );
    assert_eq!(
        parts("x [[ y ]] [[]] [[a]b]]", &[]),
        ["t:x ", "l:[[ y ]]> y ", "t: [[]] [[a]b]]"]
    );
    assert_eq!(parts("[[[a]]", &[]), ["t:", "l:[[[a]]>[a", "t:"]);
    assert_eq!(
        parts(
            "go [[Flat#s|x]] ok",
            &[("Flat#s|x", "one"), ("Flat#s|x", "two")]
        ),
        ["t:go ", "l:[[Flat#s|x]]>two", "t: ok"]
    );
    assert_eq!(parts("é[[Ünï]]", &[]), ["t:é", "l:[[Ünï]]>ünï", "t:"]);
}

#[test]
fn filters_keep_types_and_queries() {
    let mut typed = entry("note-a", "Body word", &[]);
    typed.kind = Some("user".to_owned());
    let odd = EntryView {
        kind: Some("42".to_owned()),
        ..entry("weird", "x", &[])
    };
    assert!(matches_query(&typed, "NOTE"));
    // No trim: the empty name and description leave runs of spaces.
    assert!(matches_query(&typed, "  "));
    assert!(matches_query(&typed, "body word"));
    assert!(matches_query(&odd, ""));
    assert!(!matches_query(&odd, "note"));
    assert_eq!(capitalize("unknown"), "Unknown");
    assert_eq!(capitalize("42"), "42");
}

#[test]
fn texts() {
    assert_eq!(
        below_fold_text(1),
        "1 entry is below the fold — INVISIBLE TO CLAUDE"
    );
    assert_eq!(
        below_fold_text(2),
        "2 entries are below the fold — INVISIBLE TO CLAUDE"
    );
    assert_eq!(orphan_heading(1), "1 orphan file (not in index)");
    assert_eq!(dangling_heading(3), "3 dangling pointers (file missing)");
    assert_eq!(
        create_description("", None),
        "Adds <name>.md and a MEMORY.md index line in <project>."
    );
    let done = DeleteView {
        slug: "b".to_owned(),
        dangling: vec![peektrace_protocol::view::memory::DanglingRefView {
            from: "a".to_owned(),
            target: "b".to_owned(),
            line: 1,
        }],
    };
    assert_eq!(
        deleted_toast("b", &done),
        (
            "Deleted b — 1 reference now dangling".to_owned(),
            Some("a → b".to_owned()),
            true
        )
    );
}

#[test]
fn wire_errors_read_like_the_inspector() {
    let cases = [
        (
            ApiError::FileChangedError {
                path: "/p".into(),
                reason: FileChangeReason::Mtime,
            },
            "The file changed on disk (mtime) since you loaded it.",
        ),
        (
            ApiError::MemoryValidationError {
                reason: "name must be kebab-case".into(),
                name: None,
            },
            "name must be kebab-case",
        ),
        (
            ApiError::MemoryNotFoundError {
                project: "p".into(),
                name: "b".into(),
            },
            "Memory \"b\" not found in p.",
        ),
        (
            ApiError::CapabilityUnsupportedError {
                capability_id: MEMORY_CRUD.into(),
                agent_id: AgentId::Codex,
            },
            "codex does not support memory.crud.",
        ),
        (
            ApiError::StatsRowNotFoundError {
                key: "k".into(),
                table: "t".into(),
            },
            "The request failed.",
        ),
    ];
    for (e, want) in cases {
        assert_eq!(wire_error_message(&e), want);
    }
}
