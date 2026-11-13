//! Ports of `apps/inspector/test/session-list.test.tsx`: the rail over 500
//! in-memory headers, driven through Rune's `Runtime` (no GPU, no HOME).

use peektrace_js::time::{MS_PER_DAY as DAY_MS, Utc, day_key};
use peektrace_protocol::AgentId;
use peektrace_protocol::view::{SessionHeader, SessionListingView};

fn parse_date(s: &str) -> Option<i64> {
    peektrace_js::time::parse_date(s, &Utc)
}
use peektrace_ui::state::Ui;
use peektrace_ui::views;
use rune::{Env, Runtime};

const COUNT: usize = 500;
const SIZE: (f32, f32) = (1440.0, 900.0);

fn ms(iso: &str) -> i64 {
    parse_date(iso).unwrap_or_else(|| panic!("bad date {iso}"))
}

fn iso(ms: i64) -> String {
    let rem = ms.rem_euclid(DAY_MS) / 1000;
    format!(
        "{}T{:02}:{:02}:{:02}.000Z",
        day_key(ms),
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

fn header(i: usize) -> SessionHeader {
    let cwd = format!("/home/dev/code/app-{}", i % 4);
    let n = i64::try_from(i).unwrap_or(0);
    let start = ms("2026-10-01T08:00:00.000Z") + n * 3_600_000;
    // Session 0 started first but is still being written to.
    let updated = if i == 0 {
        ms("2026-11-07T22:00:00.000Z")
    } else {
        start + 60_000
    };
    SessionHeader {
        id: format!("id-{i:04}"),
        agent: AgentId::Claude,
        path: format!("{cwd}/id-{i}.jsonl"),
        project: cwd.replace('/', "-"),
        message_count: 4,
        size_bytes: 1024,
        updated_at: iso(updated),
        cwd: Some(cwd),
        git_branch: Some(if i.is_multiple_of(2) { "HEAD" } else { "main" }.into()),
        model: Some("claude-opus-4".into()),
        title: Some(format!("Synthetic task {i}")),
        started_at: Some(iso(start)),
        cost_usd: None,
        source: None,
        source_label: None,
    }
}

#[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
fn runtime(headers: Vec<SessionHeader>, now: i64) -> Runtime<Ui> {
    let mut s = Ui::new(None, now as f64, true);
    let rid = s.sessions_rid;
    s.apply_listing(
        rid,
        Ok(SessionListingView {
            headers,
            unreadable: Vec::new(),
        }),
    );
    let mut rt = Runtime::new(s, views::view, SIZE, Env::new(true, 1.0));
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    rt
}

fn big() -> Runtime<Ui> {
    runtime((0..COUNT).map(header).collect(), ms("2026-11-08T00:00:00Z"))
}

fn texts(rt: &Runtime<Ui>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter_map(|e| e.text.or(e.label))
        .collect()
}

fn rows(rt: &Runtime<Ui>) -> Vec<rune::ElementInfo> {
    rt.elements()
        .into_iter()
        .filter(|e| {
            e.role == "link"
                && e.label
                    .as_deref()
                    .is_some_and(|l| l.starts_with("Synthetic"))
        })
        .collect()
}

#[test]
fn mounts_only_a_window_of_rows() {
    let n = rows(&big()).len();
    assert!(n > 0 && n < 100, "{n} rows mounted");
}

#[test]
fn labels_the_project_by_the_last_folder_of_cwd() {
    let rt = big();
    let all = texts(&rt);
    assert!(!all.iter().any(|t| t.contains("-home-dev-code-app-1")));
    assert!(all.iter().any(|t| t == "app-1"));
}

#[test]
fn hides_a_detached_head_branch() {
    assert!(!texts(&big()).iter().any(|t| t == "HEAD"));
}

#[test]
fn puts_the_most_recently_updated_session_first() {
    let rt = big();
    let labels: Vec<String> = rows(&rt).into_iter().filter_map(|e| e.label).collect();
    let first = labels.iter().position(|l| l == "Synthetic task 0");
    let newest_start = labels
        .iter()
        .position(|l| *l == format!("Synthetic task {}", COUNT - 1));
    assert_eq!(first, Some(0));
    assert!(newest_start.is_none_or(|n| n > 0));
}

#[test]
fn mounts_no_tooltip_per_row() {
    let rt = big();
    assert!(rt.help_tooltip().is_none());
    assert!(!rt.elements().iter().any(|e| e.role == "tooltip"));
}

#[test]
fn gives_every_row_the_same_fixed_height() {
    let rt = big();
    let rows = rows(&rt);
    assert!(!rows.is_empty());
    assert!(
        rows.iter().all(|e| (e.rect[3] - 60.0).abs() < 0.5),
        "{:?}",
        rows.iter().map(|e| e.rect[3]).collect::<Vec<_>>()
    );
}

#[test]
fn keeps_the_model_in_a_row_tooltip() {
    let rt = big();
    assert!(!rows(&rt).is_empty());
    assert!(rt.elements().iter().any(|e| {
        e.help
            .as_deref()
            .is_some_and(|h| h.ends_with(" · claude-opus-4"))
    }));
}

#[test]
fn shows_the_session_count_over_the_full_list() {
    assert!(
        texts(&big())
            .iter()
            .any(|t| t == &format!("{COUNT} sessions"))
    );
}

#[test]
fn marks_only_a_session_written_to_in_the_last_few_minutes() {
    let now = ms("2026-11-08T12:00:00Z");
    let live = SessionHeader {
        id: "live".into(),
        updated_at: iso(now - 30_000),
        ..header(1)
    };
    let idle = SessionHeader {
        id: "idle".into(),
        updated_at: iso(now - 3_600_000),
        ..header(2)
    };
    let rt = runtime(vec![idle, live], now);
    let els = rt.elements();
    let dots: Vec<usize> = els
        .iter()
        .enumerate()
        .filter(|(_, e)| e.label.as_deref() == Some("Live"))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(dots.len(), 1);
    assert_eq!(
        els[dots[0]].help.as_deref(),
        Some("Live: wrote a line in the last 5 min")
    );
    let title = els
        .iter()
        .position(|e| e.text.as_deref() == Some("Synthetic task 1"))
        .unwrap_or(usize::MAX);
    assert!(dots[0] < title);
}
