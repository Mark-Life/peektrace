//! Stats screen hazards on synthetic reports: an empty corpus, six-figure
//! cells, the 80x24 floor, and escape codes in drill hits.

mod common;

use common::{NOW_MS, config, key, text};
use peektrace_core::Agent;
use peektrace_core::stats::accumulate::{DrillHit, DrillTable, SampleRef};
use peektrace_core::stats::detectors::failures::FailRow;
use peektrace_core::stats::detectors::timing::TimeRow;
use peektrace_core::stats::detectors::waiting::WaitShapeRow;
use peektrace_core::stats::report::{DrillResult, StatsReport};
use peektrace_core::stats::schema::{Job, Kind, WaitShape};
use peektrace_tui::backend::{Backend, Request, Response};
use peektrace_tui::runtime::settle;
use peektrace_tui::theme::ColorMode;
use peektrace_tui::{App, Section};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

const ESC: char = '\u{1b}';
const BEL: char = '\u{7}';

struct Fake(StatsReport);

fn hostile() -> DrillHit {
    DrillHit {
        sample: SampleRef {
            sid: format!("{ESC}[31mabcdefghijklmnopqrstuvwxyz0123456789"),
            agent: Agent::Claude,
            project: format!("{ESC}]0;title{BEL}/tmp/p"),
            kind: Kind::Main,
            parent: None,
            seq: 1.0,
            tool_use_id: None,
            ts: Some(format!("{ESC}[Anot-a-date-at-all")),
            line: format!("{ESC}[1;32mrm -rf / tail"),
        },
        dur_sec: Some(5.0),
        tool: format!("Ba{ESC}[0msh"),
        failed: true,
        pair_tool_use_id: None,
        pair_line: None,
    }
}

impl Backend for Fake {
    fn call(&self, req: &Request) -> Response {
        match req {
            Request::Report { .. } => Response::Report(Ok(Box::new(self.0.clone()))),
            Request::Drill { .. } => Response::Drill(Ok(Box::new(DrillResult {
                table: DrillTable::Fail,
                key: "k".into(),
                total: 1,
                offset: 0.0,
                hits: vec![hostile()],
                paths: Vec::new(),
            }))),
            _ => Response::Sessions(Ok(Vec::new())),
        }
    }
}

fn empty() -> StatsReport {
    StatsReport {
        generated_at: NOW_MS,
        ..StatsReport::default()
    }
}

fn fat() -> StatsReport {
    let mut r = empty();
    r.corpus.transcripts = 3.0;
    r.corpus.bash_sec = 900.0;
    r.fails = vec![FailRow {
        key: "k".into(),
        label: "a failing thing".into(),
        r#where: "Bash".into(),
        rows: 3.0,
        sessions: 2.0,
        projects: 1.0,
        flagged: 1.0,
        note: Some("quote it".into()),
        detector: Some("zsh".into()),
        samples: Vec::new(),
    }];
    r.time = vec![TimeRow {
        key: Job::Test,
        label: "tests",
        runs: 123_456.0,
        co_present_runs: 1.0,
        total_sec: 900_000.0,
        share: 0.5,
        median_sec: 1.0,
        p95_sec: 2.0,
        capped_runs: 123_456.0,
        capped_sec: 900_000.0,
        bg_runs: 123_456.0,
        bg_sec: 900_000.0,
        samples: Vec::new(),
    }];
    r
}

fn fail(key: &str, label: &str, edit: impl FnOnce(&mut FailRow)) -> FailRow {
    let mut r = FailRow {
        key: key.into(),
        label: label.into(),
        r#where: "Bash".into(),
        rows: 0.0,
        sessions: 0.0,
        projects: 0.0,
        flagged: 0.0,
        note: None,
        detector: None,
        samples: Vec::new(),
    };
    edit(&mut r);
    r
}

/// A month of one busy corpus: two fail rows, tests and waiting.
fn busy() -> StatsReport {
    let mut r = empty();
    r.window.days = 30.0;
    r.corpus.transcripts = 2509.0;
    r.corpus.sessions = 2491.0;
    r.corpus.projects = 26.0;
    r.corpus.tool_calls = 64_644.0;
    r.corpus.bash_calls = 36_324.0;
    r.corpus.bash_sec = 85_680.0;
    r.corpus.tool_sec = 90_000.0;
    r.fails = vec![
        fail(
            "detect:zsh-equals-abort",
            "unquoted echo === separator",
            |f| {
                (f.rows, f.sessions, f.projects, f.flagged) = (169.0, 90.0, 11.0, 167.0);
                f.note = Some(
                    "quote the separator: echo \"===\" — zsh expands a bare === to a command name"
                        .into(),
                );
                f.detector = Some("zsh-equals-abort".into());
                f.samples = vec![SampleRef {
                    sid: "019fd37c-1111".into(),
                    agent: Agent::Claude,
                    project: "p".into(),
                    kind: Kind::Main,
                    parent: None,
                    seq: 1.0,
                    tool_use_id: None,
                    ts: None,
                    line: "echo ===".into(),
                }];
            },
        ),
        fail("scan:tsc", "error TS####", |f| {
            f.r#where = "Bash exit 0".into();
            (f.rows, f.sessions, f.projects) = (619.0, 217.0, 11.0);
            f.note =
                Some("591 of 619 pipe into head/tail/grep, so the exit code is the pipe's".into());
        }),
    ];
    let time = |key, label, n: [f64; 9]| TimeRow {
        key,
        label,
        runs: n[0],
        co_present_runs: n[1],
        total_sec: n[2],
        share: n[3],
        median_sec: n[4],
        p95_sec: n[5],
        capped_runs: n[6],
        capped_sec: n[7],
        bg_runs: n[8],
        bg_sec: if key == Job::Test { 21.4 } else { 2384.0 },
        samples: Vec::new(),
    };
    r.time = vec![
        time(
            Job::Test,
            "tests",
            [
                1752.0, 1764.0, 20_340.0, 0.238, 1.7, 33.5, 15.0, 6060.0, 10.0,
            ],
        ),
        time(
            Job::WaitPoll,
            "waiting",
            [
                703.0, 703.0, 18_060.0, 0.211, 4.8, 90.0, 18.0, 6480.0, 266.0,
            ],
        ),
    ];
    let w = &mut r.waiting;
    (w.calls, w.total_sec, w.share, w.sessions, w.projects) = (703.0, 18_060.0, 0.211, 150.0, 13.0);
    (w.median_sec, w.p95_sec) = (4.8, 90.0);
    (w.bg_runs, w.bg_sec, w.bg_declared_sec) = (266.0, 2384.0, 15_300.0);
    w.by_shape = vec![WaitShapeRow {
        key: WaitShape::LoopPoll,
        calls: 217.0,
        total_sec: 12_900.0,
        share: 0.7,
    }];
    r.caveats = vec![
        "durSec is the gap between a call and its result: it includes harness overhead".into(),
        "silent-failure counts are a floor: a detector only sees stored output".into(),
    ];
    r
}

fn open(report: StatsReport, w: u16, h: u16, keys: &[&str]) -> String {
    let backend = Fake(report);
    let mut app = App::new(config(false, ColorMode::TrueColor), Rect::new(0, 0, w, h));
    settle(&mut app, &backend);
    app.switch(Section::Stats);
    settle(&mut app, &backend);
    let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
    app.draw(&mut buf);
    for k in keys {
        app.key(key(k));
        settle(&mut app, &backend);
        buf = Buffer::empty(Rect::new(0, 0, w, h));
        app.draw(&mut buf);
    }
    text(&buf)
}

fn widest(frame: &str) -> usize {
    frame.lines().map(|l| l.chars().count()).max().unwrap_or(0)
}

#[test]
fn an_empty_corpus_renders_both_tables_and_divides_by_nothing() {
    let frame = open(empty(), 100, 44, &[]);
    assert!(frame.contains("No failing calls"), "{frame}");
    assert!(frame.contains("No shell seconds"));
    assert!(!frame.contains("NaN"));
    assert!(widest(&frame) <= 100);
}

#[test]
fn six_figure_capped_and_backgrounded_counts_keep_the_row_on_one_line() {
    let frame = open(fat(), 100, 44, &[]);
    let row = frame
        .lines()
        .find(|l| l.contains("tests"))
        .expect("time row");
    assert!(row.contains("123,456"), "{row}");
    assert!(widest(&frame) <= 100);
}

#[test]
fn the_narrow_layout_fits_80x24_without_wrapping() {
    let frame = open(fat(), 80, 24, &[]);
    assert!(frame.contains("WHAT COSTS TIME"), "{frame}");
    assert!(widest(&frame) <= 80);
}

#[test]
fn below_the_floor_says_so_instead_of_scrambling() {
    let frame = open(fat(), 60, 20, &[]);
    assert!(frame.contains("stats needs 80×24"));
    assert!(!frame.contains("WHAT FAILS"));
}

#[test]
fn no_escape_sequence_from_a_transcript_reaches_the_drill() {
    let frame = open(fat(), 100, 44, &["<tab>", "<enter>"]);
    assert!(frame.contains("DRILL  --fail k"), "{frame}");
    assert!(frame.contains("rm -rf / tail"));
    assert!(!frame.contains(ESC));
    assert!(!frame.contains(BEL));
    assert!(widest(&frame) <= 100);
}

#[test]
fn a_finding_title_never_runs_into_a_wide_impact() {
    let mut r = fat();
    r.fails[0].label = "shell output too large: persisted".into();
    r.fails[0].sessions = 123_456_789.0;
    let frame = open(r, 160, 48, &[]);
    let row = frame
        .lines()
        .find(|l| l.contains("shell output"))
        .expect("finding");
    assert!(
        row.contains("… 123,456,789 sessions") || row.contains("persisted 123,456,789"),
        "{row}"
    );
}

#[test]
fn both_ranked_tables_render_with_their_ranking_keys_at_100_columns() {
    let frame = open(busy(), 100, 44, &[]);
    for want in [
        "WHAT COSTS TIME",
        "FIX FIRST",
        "ranked by sessions spanned",
        "SESSIONS",
        "TIME",
        "unquoted echo === separator",
        "waiting",
        "2,509 transcripts",
        "NOTE",
    ] {
        assert!(frame.contains(want), "{want}: {frame}");
    }
    assert!(widest(&frame) <= 100);
}

#[test]
fn eighty_columns_drop_the_wide_columns_instead_of_overflowing() {
    let frame = open(busy(), 80, 42, &[]);
    assert!(frame.contains("WHAT COSTS TIME"), "{frame}");
    assert!(frame.contains("WHAT FAILS"));
    assert!(!frame.contains("WHERE"), "{frame}");
    assert!(!frame.contains("MEDIAN"), "{frame}");
    assert!(widest(&frame) <= 80);
}
