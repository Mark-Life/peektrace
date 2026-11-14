//! Every pane that waits on a first load shows a skeleton, not a line of
//! text: the session detail, Stats, Memory, Settings and Capabilities.

mod common;

use common::tree::blank_leaves_under;
use peektrace_ui::state::{Route, Ui};
use peektrace_ui::views;
use rune::{Env, Runtime};

fn runtime(s: Ui) -> Runtime<Ui> {
    let mut rt = Runtime::new(s, views::view, (1440.0, 900.0), Env::new(true, 1.0));
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    rt
}

/// Skeleton blocks under `key`, which must be on screen.
fn blocks(rt: &Runtime<Ui>, key: &str) -> usize {
    assert!(rt.find(key).is_some(), "no {key}");
    blank_leaves_under(&rt.elements(), key)
}

fn has_status(rt: &Runtime<Ui>, text: &str) -> bool {
    rt.elements()
        .iter()
        .any(|e| e.role == "status" && e.text.as_deref() == Some(text))
}

#[test]
fn a_session_being_analysed_shows_the_detail_skeleton() {
    let mut s = Ui::new(None, 0.0, true);
    s.select("11111111-1111-4111-8111-111111111111", None);
    let rt = runtime(s);
    let n = blocks(&rt, "detail-pending");
    assert!(n > 10, "{n} skeleton blocks");
    assert!(has_status(&rt, "Loading session…"));
    assert!(rt.find("verdict").is_none());
}

#[test]
fn memory_loading_shows_its_skeleton() {
    let mut s = Ui::new(None, 0.0, true);
    s.go(Route::Memory);
    let rt = runtime(s);
    let n = blocks(&rt, "mem-loading");
    assert!(n > 5, "{n} skeleton blocks");
    assert!(has_status(&rt, "Loading memory…"));
}

#[test]
fn settings_and_capabilities_loading_show_the_result_skeleton() {
    for route in [Route::Settings, Route::Capabilities] {
        let mut s = Ui::new(None, 0.0, true);
        s.go(route);
        let rt = runtime(s);
        assert!(blocks(&rt, "result-pending") >= 3, "{route:?}");
    }
}
