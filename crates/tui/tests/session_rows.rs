//! Session cards with long titles, projects and branches at a narrow
//! width: every part stays in its card, and the worktree row shows its
//! parent project, branch and marker.

mod common;

use common::*;
use peektrace_core::Agent;
use peektrace_core::model::{Launcher, ProjectSlug, SessionHeader, SessionId, Worktree};
use peektrace_core::sessions::model::{AnalyzedSession, ParsedSession, Provider};
use peektrace_tui::App;
use peektrace_tui::backend::{Backend, Request, Response};
use peektrace_tui::runtime::settle;
use peektrace_tui::theme::ColorMode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

const WT: &str = "/Users/demo/.t3/worktrees/a-very-long-project-name/t3-00000000-0000000000000000";

fn launcher() -> Launcher {
    Launcher {
        name: "t3code".to_owned(),
        branch: None,
        worktree_path: Some(WT.to_owned()),
        parent_title: Some("Plan the session list rework".to_owned()),
    }
}

fn worktree() -> Worktree {
    Worktree {
        project: "a-very-long-project-name".to_owned(),
        project_root: Some("/Users/demo/code/a-very-long-project-name".to_owned()),
        path: WT.to_owned(),
        branch: Some("t3/session-rows-logos-worktree-labels".to_owned()),
    }
}

fn header(id: &str, model: &str) -> SessionHeader {
    SessionHeader {
        id: SessionId::new(id),
        agent: Agent::Claude,
        path: format!("/{id}"),
        project: ProjectSlug::new("-Users-demo-x"),
        message_count: 12_345,
        size_bytes: 100,
        updated_at: "2026-11-17T10:00:00.000Z".to_owned(),
        cwd: Some(WT.to_owned()),
        git_branch: Some("HEAD".to_owned()),
        model: Some(model.to_owned()),
        title: Some(
            "Make every session row fit its card even when the title runs on and on".to_owned(),
        ),
        started_at: Some("2026-11-17T09:30".to_owned()),
        malformed_lines: None,
        cost: peektrace_core::HeaderCost::default(),
        cost_parts: None,
        source: None,
        launcher: Some(launcher()),
        models: vec![model.to_owned()],
        worktree: Some(worktree()),
    }
}

/// A plain row: no launcher, no worktree, an unknown model, a wide-char
/// project and a long branch.
fn plain() -> SessionHeader {
    SessionHeader {
        cwd: Some("/Users/demo/日本語のとても長いプロジェクト名".to_owned()),
        git_branch: Some("feature/an-extremely-long-branch-name-that-never-ends".to_owned()),
        launcher: None,
        worktree: None,
        model: Some("nonexistent-model-xyz".to_owned()),
        models: Vec::new(),
        agent: Agent::Pi,
        ..header("b", "x")
    }
}

struct Long;

impl Backend for Long {
    fn call(&self, req: &Request) -> Response {
        match req {
            Request::Analyze { .. } => Response::Analyze(Ok(Box::new(analyzed()))),
            _ => Response::Sessions(Ok(vec![header("a", "claude-opus-5-5"), plain()])),
        }
    }
}

fn analyzed() -> AnalyzedSession {
    AnalyzedSession {
        parsed: ParsedSession {
            provider: Provider::ClaudeCode,
            session_id: "abcd1234".to_owned(),
            title: Some("Worktree session".to_owned()),
            models: vec![
                "claude-opus-5-5".to_owned(),
                "nvidia/GLM-5.3-Flash-NVFP4".to_owned(),
            ],
            cwd: Some(WT.to_owned()),
            git_branch: Some("HEAD".to_owned()),
            launcher: Some(launcher()),
            worktree: Some(worktree()),
            ..ParsedSession::default()
        },
        context_window: 200_000.0,
        ..AnalyzedSession::default()
    }
}

fn draw(w: u16, h: u16) -> String {
    let area = Rect::new(0, 0, w, h);
    let mut app = App::new(config(false, ColorMode::TrueColor), area);
    settle(&mut app, &Long);
    let mut buf = Buffer::empty(area);
    app.draw(&mut buf);
    let mut buf = Buffer::empty(area);
    app.draw(&mut buf);
    text(&buf)
}

#[test]
fn long_rows_stay_inside_their_cards_at_a_narrow_width() {
    let t = draw(80, 30);
    check("sessions-long-rows-80x30.txt", &t);
    assert!(t.contains("T3 CC An ⎇ a-ver… · t3/se…"), "{t}");
    assert!(t.contains("12345 msgs"), "{t}");
    assert!(t.contains("Nov 17, 09:30"), "{t}");
    assert!(
        !t.contains("t3-00000000 ·"),
        "the leaf folder is never the project"
    );
    assert!(t.contains("Pi ? 日本"), "{t}");
}

#[test]
fn the_detail_names_every_source_part_in_full() {
    let t = draw(220, 40);
    assert!(
        t.contains("t3code › Claude Code · Anthropic · claude-opus-5-5"),
        "{t}"
    );
    assert!(t.contains("Anthropic · claude-opus-5-5"), "{t}");
    assert!(t.contains("NVIDIA · nvidia/GLM-5.3-Flash-NVFP4"), "{t}");
    assert!(
        t.contains(&format!("{WT} · t3/session-rows-logos-worktree-labels")),
        "{t}"
    );
    assert!(
        t.contains(&format!("⎇ a-very-long-project-name · {WT}")),
        "{t}"
    );
    assert!(
        t.contains("Started by     Plan the session list rework"),
        "{t}"
    );
}
