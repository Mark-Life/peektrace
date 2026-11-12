//! The Sessions screen over a fake backend with synthetic analyses: history
//! expand and sort, attachment badges, card selection driving the detail,
//! the estimated cost, and the growth chart toggle.

mod common;

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use common::*;
use peektrace_core::Agent;
use peektrace_core::model::{ProjectSlug, SessionHeader, SessionId};
use peektrace_core::sessions::model::{
    AnalyzedSession, BudgetKey, BudgetSlice, BudgetSlices, EventKind, EventShape, ParsedSession,
    SessionCost, SubagentRef, TimelineEvent, Turn, TurnSnapshot, TurnTokens, TurnsCost,
};
use peektrace_core::watch::WatchVersions;
use peektrace_tui::App;
use peektrace_tui::backend::{Backend, Request, Response};
use peektrace_tui::runtime::settle;
use peektrace_tui::theme::ColorMode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

const ESC: char = '\u{1b}';

type Analyzed = fn(&str) -> AnalyzedSession;

struct Fake {
    analyzed: Analyzed,
}

fn header(id: &str, agent: Agent, title: &str, started: &str) -> SessionHeader {
    SessionHeader {
        id: SessionId::new(id),
        agent,
        path: format!("/{id}"),
        project: ProjectSlug::new("p"),
        message_count: 3,
        size_bytes: 100,
        updated_at: format!("{started}:00.000Z"),
        cwd: None,
        git_branch: None,
        model: None,
        title: Some(title.to_owned()),
        started_at: Some(started.to_owned()),
        malformed_lines: None,
        cost_usd: None,
        unpriced_models: None,
        source: None,
    }
}

impl Backend for Fake {
    fn call(&self, req: &Request) -> Response {
        match req {
            Request::Analyze { id, .. } => {
                Response::Analyze(Ok(Box::new((self.analyzed)(&format!("Analyzed-{id}")))))
            }
            _ => Response::Sessions(Ok(vec![
                header("a", Agent::Claude, "Alpha", "2026-07-02T10:00"),
                header("b", Agent::Codex, "Beta", "2026-07-01T09:00"),
            ])),
        }
    }
}

fn event(
    index: usize,
    kind: EventKind,
    title: &str,
    preview: &str,
    body: &str,
    est: f64,
) -> TimelineEvent {
    let mut e = TimelineEvent::new(EventShape::Prompt, index, kind).text(
        title.to_owned(),
        preview.to_owned(),
        body.to_owned(),
        est,
    );
    e.request_id = Some("r1".to_owned());
    e
}

/// A Claude session with one Bash call and its result.
fn fake_analyzed(title: &str) -> AnalyzedSession {
    let mut call = event(
        0,
        EventKind::ToolCall,
        "Bash",
        &format!("{ESC}[32mBash tool call{ESC}[0m"),
        r#"{"command":"ls -la /tmp"}"#,
        10.0,
    );
    call.tool_name = Some("Bash".to_owned());
    let result = event(
        1,
        EventKind::ToolResult,
        "result",
        "total 8",
        "total 8\nfile.txt",
        5.0,
    );
    AnalyzedSession {
        parsed: ParsedSession {
            session_id: "abcd1234".to_owned(),
            path: "/x".to_owned(),
            title: Some(title.to_owned()),
            models: vec!["claude".to_owned()],
            cwd: Some("/repo".to_owned()),
            events: vec![call, result],
            turns: vec![Turn::new(
                "r1".to_owned(),
                "claude".to_owned(),
                TurnTokens::default(),
                None,
            )],
            ..ParsedSession::default()
        },
        context_window: 200_000.0,
        peak_context_tokens: 50_000.0,
        final_context_tokens: 40_000.0,
        total_output_tokens: 1000.0,
        system_overhead_tokens: 2000.0,
        peak_cache_read_tokens: 10_000.0,
        dumb_zone_cross_turn: -1,
        dumb_zone_fraction: 0.4,
        turn_count: 1,
        user_message_count: 1,
        tool_call_count: 1,
        ..AnalyzedSession::default()
    }
}

/// The same shape, its last event the biggest so size order flips the rows.
fn heavy_tail(title: &str) -> AnalyzedSession {
    let mut s = fake_analyzed(title);
    s.parsed.events[1].tokens_est = 500.0;
    s
}

/// The same shape, its timeline three attachments a badge must tell apart.
fn attachments(title: &str) -> AnalyzedSession {
    let mut s = fake_analyzed(title);
    let attachment = |index, ty: &str, preview: &str| {
        let mut e = event(index, EventKind::Attachment, ty, preview, preview, 8.0);
        e.attachment_type = (!ty.is_empty()).then(|| ty.to_owned());
        e
    };
    s.parsed.events = vec![
        attachment(0, "skill_listing", "- agent-to-human: Use when …"),
        attachment(1, "opened_file_in_ide", "drawer.tsx"),
        attachment(2, "", "no type at all"),
    ];
    s
}

fn priced(title: &str) -> AnalyzedSession {
    AnalyzedSession {
        cost: Some(SessionCost {
            from_agent: false,
            total_usd: Some(1.23),
            unpriced_models: Some(vec!["mystery-model".to_owned()]),
            ..SessionCost::default()
        }),
        ..fake_analyzed(title)
    }
}

struct Ui {
    app: App,
    backend: Fake,
    area: Rect,
}

impl Ui {
    fn new(analyzed: Analyzed, w: u16, h: u16) -> Self {
        let area = Rect::new(0, 0, w, h);
        let backend = Fake { analyzed };
        let mut app = App::new(config(false, ColorMode::TrueColor), area);
        settle(&mut app, &backend);
        let mut me = Self { app, backend, area };
        me.text();
        me
    }

    fn text(&mut self) -> String {
        let mut buf = Buffer::empty(self.area);
        self.app.draw(&mut buf);
        text(&buf)
    }

    fn keys(&mut self, keys: &[&str]) -> String {
        for k in keys {
            self.app.key(key(k));
            settle(&mut self.app, &self.backend);
            self.text();
        }
        self.text()
    }
}

#[test]
fn history_expands_to_a_highlighted_body_and_strips_ansi() {
    let mut ui = Ui::new(fake_analyzed, 80, 24);
    let collapsed = ui.keys(&["<enter>"]);
    assert!(collapsed.contains("Bash tool"), "{collapsed}");
    assert!(!collapsed.contains(ESC));
    assert!(!collapsed.contains("ls -la /tmp"));
    let expanded = ui.keys(&["<enter>"]);
    assert!(expanded.contains("ls -la /tmp"), "{expanded}");
}

#[test]
fn s_sorts_the_history_biggest_first_and_keeps_open_rows_open() {
    let mut ui = Ui::new(heavy_tail, 160, 24);
    let ordered = ui.keys(&["<enter>"]);
    let at = |t: &str, needle: &str| t.find(needle).unwrap_or(usize::MAX);
    assert!(
        at(&ordered, "Bash tool") < at(&ordered, "total 8"),
        "{ordered}"
    );
    assert!(ui.keys(&["j", "<enter>"]).contains("file.txt"));
    let sorted = ui.keys(&["s"]);
    assert!(sorted.contains("~500   0.3%"), "{sorted}");
    assert!(
        at(&sorted, "total 8") < at(&sorted, "Bash tool"),
        "{sorted}"
    );
    assert!(sorted.contains("file.txt"));
}

#[test]
fn attachment_rows_badge_their_type_not_the_bare_kind() {
    let t = Ui::new(attachments, 100, 24).text();
    assert!(t.contains("skill listing"), "{t}");
    assert!(t.contains("opened file in ide"), "{t}");
    assert!(t.contains("[attachment]"), "{t}");
}

#[test]
fn session_cards_render_and_keyboard_selection_drives_the_detail() {
    let mut ui = Ui::new(fake_analyzed, 120, 34);
    let first = ui.text();
    assert!(first.contains("Analyzed-a"), "{first}");
    assert!(first.contains("Alpha"));
    assert!(first.contains("Beta"));
    assert!(first.contains("filter (press /)"));
    assert!(first.contains("agent: all · a to cycle"));
    let next = ui.keys(&["<down>"]);
    assert!(next.contains("Analyzed-b"), "{next}");
}

#[test]
fn session_detail_summary_shows_the_estimated_cost() {
    let t = Ui::new(priced, 140, 40).text();
    assert!(t.contains("Est. cost"), "{t}");
    assert!(t.contains("~$1.23 · unpriced: mystery-model"), "{t}");
}

/// Five turns climbing to 190K of a 200K window, compacted at turn 3.
fn chart_demo(_: &str) -> AnalyzedSession {
    const CTXS: [f64; 5] = [30_000.0, 80_000.0, 140_000.0, 190_000.0, 160_000.0];
    let snap = |i: usize, ctx: f64| TurnSnapshot {
        turn_index: i,
        model: "c".to_owned(),
        ctx,
        output_tokens: 2000.0,
        cache_read_tokens: 0.0,
        slices: BudgetSlices {
            system_tools: 40_000.0,
            prompts: (ctx * 0.2).round(),
            tool_results: (ctx * 0.3).round(),
            unattributed: (ctx - 40_000.0 - (ctx * 0.5).round()).max(0.0),
            ..BudgetSlices::default()
        },
        ts: None,
    };
    AnalyzedSession {
        parsed: ParsedSession {
            session_id: "a".to_owned(),
            path: "/x".to_owned(),
            title: Some("Chart demo".to_owned()),
            models: vec!["c".to_owned()],
            cwd: Some("/r".to_owned()),
            turns: (0..CTXS.len())
                .map(|i| Turn::new(format!("r{i}"), "c".to_owned(), TurnTokens::default(), None))
                .collect(),
            ..ParsedSession::default()
        },
        budget: [
            (BudgetKey::SystemTools, "#6e7681", "System"),
            (BudgetKey::Prompts, "#3fb950", "Prompts"),
            (BudgetKey::ToolResults, "#f0883e", "Tool results"),
        ]
        .into_iter()
        .map(|(key, color, label)| BudgetSlice {
            key,
            label,
            short: key.as_str(),
            color,
            estimated: false,
            note: None,
            tokens: 1.0,
        })
        .collect(),
        compaction_turns: vec![3],
        snapshots: CTXS.iter().enumerate().map(|(i, c)| snap(i, *c)).collect(),
        context_window: 200_000.0,
        peak_context_tokens: 190_000.0,
        peak_turn_index: 3,
        final_context_tokens: 160_000.0,
        total_output_tokens: 8000.0,
        system_overhead_tokens: 40_000.0,
        dumb_zone_cross_turn: 2,
        dumb_zone_fraction: 0.4,
        dumb_zone_turns: 3,
        turn_count: 5,
        user_message_count: 1,
        tool_call_count: 3,
        ..AnalyzedSession::default()
    }
}

#[test]
fn g_toggles_the_analysis_pane_to_the_growth_chart() {
    let mut ui = Ui::new(chart_demo, 116, 38);
    assert!(ui.text().contains("Full history"), "{}", ui.text());
    let t = ui.keys(&["g"]);
    assert!(t.contains("Context growth"), "{t}");
    assert!(t.contains("200.0K"), "{t}");
    assert!(t.contains('█'), "{t}");
    assert!(t.contains("Tool results"), "{t}");
}

/// Counts analyses of a session with one subagent; the subagent files'
/// fingerprint is whatever `stamp` holds.
struct Nested {
    analyses: AtomicUsize,
    stamp: AtomicU64,
}

impl Backend for Nested {
    fn call(&self, req: &Request) -> Response {
        match req {
            Request::Analyze { .. } => {
                self.analyses.fetch_add(1, Ordering::SeqCst);
                let mut s = fake_analyzed("Nested");
                s.parsed.subagents = vec![SubagentRef {
                    id: "agent-1".to_owned(),
                    path: "/p/a/subagents/agent-1.jsonl".to_owned(),
                    agent_type: None,
                    description: None,
                    tool_use_id: None,
                    parent_agent_id: None,
                    workflow_run_id: None,
                    workflow_phase: None,
                    workflow_phase_index: None,
                    tool_use_id_linked: false,
                    turns: 1,
                    peak_context_tokens: 0.0,
                    cost: TurnsCost::default(),
                    unreadable: false,
                }];
                Response::Analyze(Ok(Box::new(s)))
            }
            Request::Stamp { dirs } => {
                assert_eq!(dirs, &["/p/a/subagents".to_owned()]);
                Response::Stamp(Ok(self.stamp.load(Ordering::SeqCst)))
            }
            _ => Response::Sessions(Ok(vec![header(
                "a",
                Agent::Claude,
                "Alpha",
                "2026-07-02T10:00",
            )])),
        }
    }
}

#[test]
fn a_session_with_subagents_re_analyzes_only_when_they_change() {
    let backend = Nested {
        analyses: AtomicUsize::new(0),
        stamp: AtomicU64::new(1),
    };
    let mut app = App::new(
        config(false, ColorMode::TrueColor),
        Rect::new(0, 0, 160, 48),
    );
    settle(&mut app, &backend);
    let tick = |app: &mut App, n: u64| {
        app.watch(WatchVersions {
            memory: 0,
            sessions: n,
        });
        settle(app, &backend);
    };
    assert_eq!(backend.analyses.load(Ordering::SeqCst), 1);
    for n in 1..=5 {
        tick(&mut app, n);
    }
    assert_eq!(
        backend.analyses.load(Ordering::SeqCst),
        1,
        "unchanged files, no new analysis"
    );
    backend.stamp.store(2, Ordering::SeqCst);
    tick(&mut app, 6);
    assert_eq!(
        backend.analyses.load(Ordering::SeqCst),
        2,
        "a subagent file changed"
    );
    tick(&mut app, 7);
    assert_eq!(backend.analyses.load(Ordering::SeqCst), 2);
}

/// Reads the result body with its secret masked or shown, as asked. The
/// token is as long as the secret and sits mid-body, past the hashed ends.
struct Redacting;

impl Backend for Redacting {
    fn call(&self, req: &Request) -> Response {
        match req {
            Request::Analyze { redact, .. } => {
                let secret = if *redact {
                    "[REDACTED:credential]"
                } else {
                    "hunter2hunter2hunter2"
                };
                let pad = "x".repeat(400);
                let mut s = fake_analyzed("Alpha");
                s.parsed.events[1].body = format!("{pad}\npassword = {secret}\n{pad}");
                Response::Analyze(Ok(Box::new(s)))
            }
            _ => Response::Sessions(Ok(vec![header(
                "a",
                Agent::Claude,
                "Alpha",
                "2026-07-02T10:00",
            )])),
        }
    }
}

#[test]
fn r_swaps_an_open_body_between_redacted_and_revealed() {
    let area = Rect::new(0, 0, 160, 48);
    let mut app = App::new(config(false, ColorMode::TrueColor), area);
    let draw = |app: &mut App| {
        let mut buf = Buffer::empty(area);
        app.draw(&mut buf);
        text(&buf)
    };
    settle(&mut app, &Redacting);
    for k in ["<enter>", "j", "<enter>", "<left>"] {
        app.key(key(k));
        settle(&mut app, &Redacting);
        draw(&mut app);
    }
    let flip = |app: &mut App| {
        app.key(key("r"));
        // Drawn while the new analysis is in flight, from the old one.
        draw(app);
        settle(app, &Redacting);
        draw(app)
    };
    let revealed = flip(&mut app);
    assert!(revealed.contains("password = hunter2"), "{revealed}");
    let hidden = flip(&mut app);
    assert!(hidden.contains("password = [REDACTED"), "{hidden}");
    assert!(!hidden.contains("hunter2"), "{hidden}");
}
