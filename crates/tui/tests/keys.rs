//! Key-sequence tests for the state machine: global keys and the typing
//! gate, each screen's modes, kept state, live refresh and stale replies.

mod common;

use std::fs::OpenOptions;
use std::io::Write as _;

use common::*;
use peektrace_core::watch::WatchVersions;
use peektrace_tui::Section;
use peektrace_tui::backend::{Backend, Reply};
use peektrace_tui::theme::ColorMode;

fn sessions() -> Harness {
    Harness::new(build_home(), 160, 48)
}

#[test]
fn quit_keys_and_the_typing_gate() {
    let mut h = sessions();
    h.keys(&["/", "q", "1", "5"]);
    assert!(!h.app.quit(), "q types into the filter");
    assert_eq!(h.app.section, Section::Sessions, "digits type too");
    assert!(h.text().contains("/ q15"), "{}", h.text());
    h.keys(&["<esc>", "q"]);
    assert!(h.app.quit());
    let mut h = sessions();
    h.keys(&["/", "<ctrl-c>"]);
    assert!(h.app.quit(), "Ctrl+C quits even while typing");
}

#[test]
fn sections_switch_and_keep_their_state() {
    let mut h = sessions();
    h.keys(&["j", "/", "r", "e", "<enter>"]);
    let before = h.text();
    assert!(before.contains("/ re"), "{before}");
    h.keys(&["5", "2", "1"]);
    assert_eq!(h.app.section, Section::Sessions);
    assert_eq!(h.text(), before, "selection and filter survive a switch");
}

#[test]
fn reopening_the_filter_shows_the_query() {
    let mut h = sessions();
    h.keys(&["/", "f", "i", "x", "<enter>"]);
    assert!(h.text().contains("Sessions (1/8)"));
    h.keys(&["/"]);
    assert!(h.text().contains("/ fix"), "reopened input keeps the text");
    h.keys(&["<bs>", "<bs>", "<bs>", "z", "z", "z", "<esc>"]);
    assert!(h.text().contains("No sessions match the filter."));
}

#[test]
fn tab_flips_focus_while_the_filter_stays_open() {
    let mut h = sessions();
    h.keys(&["/", "<tab>", "a"]);
    assert!(h.app.sessions.filtering);
    assert!(h.text().contains("/ a"), "keys still reach the input");
    assert_eq!(
        h.app.sessions.focus,
        peektrace_tui::screens::sessions::Focus::Detail
    );
}

#[test]
fn agent_facet_cycles_in_id_order() {
    let mut h = sessions();
    let seen: Vec<String> = (0..4)
        .map(|_| {
            h.keys(&["a"]);
            h.text()
                .lines()
                .find(|l| l.contains("agent:"))
                .unwrap_or("")
                .to_owned()
        })
        .collect();
    assert!(seen[0].contains("agent: Claude"));
    assert!(seen[1].contains("agent: Codex"));
    assert!(seen[2].contains("agent: Pi"));
    assert!(seen[3].contains("agent: all"));
}

#[test]
fn redaction_toggles_from_the_list() {
    let mut h = sessions();
    h.keys(&["r"]);
    let t = h.text();
    assert!(t.contains("⚠ Redaction OFF"));
    assert!(t.contains("r hide"));
}

#[test]
fn expanding_a_bash_row_highlights_and_strips_escapes() {
    let mut h = sessions();
    h.keys(&["<enter>", "j", "j", "j", "<enter>"]);
    let t = h.text();
    assert!(t.contains("npm test -- --grep case1"), "{t}");
    h.keys(&["j", "<enter>"]);
    let t = h.text();
    assert!(t.contains("FAIL case1"), "{t}");
    assert!(t.contains("link done"));
    assert!(!t.contains('\u{1b}'));
    assert!(!t.contains("]8;;"));
}

#[test]
fn biggest_first_keeps_open_rows_open() {
    let mut h = sessions();
    h.keys(&["<enter>", "<enter>"]);
    assert!(
        h.text()
            .contains("Fix the flaky build and tidy the scripts.")
    );
    let open = h
        .text()
        .matches("Fix the flaky build and tidy the scripts.")
        .count();
    h.keys(&["s"]);
    let t = h.text();
    assert!(t.contains('%'), "share column in size order");
    h.keys(&["s"]);
    assert_eq!(
        h.text()
            .matches("Fix the flaky build and tidy the scripts.")
            .count(),
        open
    );
}

#[test]
fn expand_all_then_collapse_all() {
    let mut h = sessions();
    h.keys(&["<enter>", "E"]);
    assert!(
        h.text().contains("Step 1: checking with `Bash`.\n")
            || h.text().matches("Step 1: checking").count() >= 2
    );
    h.keys(&["C"]);
    assert_eq!(h.text().matches("Step 1: checking").count(), 1);
}

#[test]
fn history_state_resets_on_another_session() {
    let mut h = sessions();
    h.keys(&["<enter>", "E", "s", "<esc>", "j"]);
    let t = h.text();
    let rows: Vec<&str> = t.lines().filter(|l| l.contains("│ [t")).collect();
    assert!(
        rows[0].contains("[t0] [user-prompt]"),
        "transcript order, not size order: {t}"
    );
    assert!(!rows[0].contains('%'), "no share column");
    let at = t
        .lines()
        .position(|l| l.contains("[t0] [user-prompt]"))
        .expect("row");
    assert!(
        !t.lines().nth(at + 1).unwrap_or("").contains("why is"),
        "no row open"
    );
}

#[test]
fn g_toggles_the_growth_chart_from_the_list() {
    let mut h = sessions();
    h.keys(&["g"]);
    let t = h.text();
    assert!(t.contains("Context growth 150 turns"), "{t}");
    assert!(
        t.contains("    turn 1"),
        "turn label is apart from the first tick: {t}"
    );
    assert!(t.contains("◆"));
    h.keys(&["j"]);
    assert!(
        h.text().contains("Context growth"),
        "the chart stays while the list moves"
    );
    h.keys(&["g"]);
    assert!(h.text().contains("Full history"));
}

#[test]
fn a_stale_reply_is_dropped() {
    let mut h = sessions();
    h.app.key(key("j"));
    let first = h.app.take_jobs();
    h.app.key(key("j"));
    let second = h.app.take_jobs();
    let reply = |job: &peektrace_tui::backend::Job| Reply {
        id: job.id,
        slot: job.req.slot(),
        resp: h.backend.call(&job.req),
    };
    let id_of = |job: &peektrace_tui::backend::Job| match &job.req {
        peektrace_tui::backend::Request::Analyze { id, .. } => id[..8].to_owned(),
        other => panic!("{other:?}"),
    };
    let (old, new) = (id_of(&first[0]), id_of(&second[0]));
    let (a, b) = (reply(&first[0]), reply(&second[0]));
    h.app.reply(b);
    h.app.reply(a);
    let t = h.text();
    assert!(
        t.contains(&format!("{new} ·")),
        "the newer reply shows: {t}"
    );
    assert!(
        !t.contains(&format!("{old} ·")),
        "the older reply is dropped: {t}"
    );
    assert!(!t.contains("Analysis ↻"));
}

#[test]
fn a_growing_session_re_analyzes_and_keeps_open_rows() {
    let mut h = sessions();
    h.keys(&["<enter>", "<enter>"]);
    assert!(h.text().contains("468 events"));
    let path = h
        .home
        .join(format!(".claude/projects/-Users-demo-long/{LONG_ID}.jsonl"));
    let mut f = OpenOptions::new().append(true).open(path).expect("open");
    writeln!(
        f,
        r#"{{"type":"user","timestamp":"2026-11-11T11:00:00.000Z","message":{{"role":"user","content":"one more thing"}}}}"#
    )
    .expect("append");
    h.app.watch(WatchVersions {
        memory: 0,
        sessions: 1,
    });
    h.settle();
    let t = h.text();
    assert!(t.contains("469 events"), "{t}");
    assert_eq!(
        t.matches("Fix the flaky build and tidy the scripts.")
            .count(),
        2,
        "row 0 stays open"
    );
}

#[test]
fn a_hidden_screen_refetches_only_after_a_change() {
    let mut h = sessions();
    let lists = h.backend.count("Sessions");
    h.keys(&["2", "1"]);
    assert_eq!(h.backend.count("Sessions"), lists, "no change, no refetch");
    h.keys(&["2"]);
    h.app.watch(WatchVersions {
        memory: 0,
        sessions: 1,
    });
    h.settle();
    assert_eq!(
        h.backend.count("Sessions"),
        lists,
        "hidden screens do not fetch"
    );
    h.keys(&["1"]);
    assert_eq!(h.backend.count("Sessions"), lists + 1);
}

#[test]
fn memory_search_type_filter_and_body_focus() {
    let mut h = sessions();
    h.keys(&["2", "j", "j", "j", "<tab>"]);
    assert!(h.app.memory.body_focused);
    assert!(h.text().contains("tab list"));
    h.keys(&["<tab>", "/", "p", "u", "r", "e", "<enter>"]);
    let t = h.text();
    assert!(t.contains("Memory (2)"), "{t}");
    h.keys(&["/"]);
    assert!(h.text().contains("/ pure"));
    h.keys(&["<esc>", "t"]);
    assert!(h.text().contains("type:user"));
    h.keys(&["t", "t", "t", "t"]);
    let t = h.text();
    assert!(t.contains("type:untyped"));
    assert!(t.contains("No memories match the current filters."), "{t}");
}

#[test]
fn settings_edit_add_remove_and_save() {
    let mut h = sessions();
    h.keys(&["4", "<enter>"]);
    assert!(h.app.settings.typing(), "enter edits the path");
    h.keys(&["<end>", "2", "<enter>", "x", "<enter>"]);
    let t = h.text();
    assert!(t.contains("~/work/.claude2"), "{t}");
    assert!(t.contains("workx"));
    h.keys(&["j", "a"]);
    h.keys(&["~", "/", "n", "<esc>", "d", "<ctrl-s>"]);
    let t = h.text();
    assert!(
        t.contains("Saved — restart `peektrace serve` to apply."),
        "{t}"
    );
    let file = std::fs::read_to_string(h.home.join(".peektrace/settings.json")).expect("settings");
    assert!(
        file.contains("~/work/.claude2") && file.contains("workx"),
        "{file}"
    );
}

#[test]
fn settings_conflict_shows_the_banner_and_reloads() {
    let mut h = sessions();
    h.keys(&["4"]);
    let path = h.home.join(".peektrace/settings.json");
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&path, "{\"roots\":{\"pi\":[{\"path\":\"~/pi2\"}]}}\n").expect("write");
    h.keys(&["s"]);
    let t = h.text();
    assert!(t.contains("settings.json changed on disk."), "{t}");
    assert!(t.contains("reload (discard edits)"));
    h.keys(&["r"]);
    let t = h.text();
    assert!(!t.contains("changed on disk"));
    assert!(t.contains("~/pi2"), "{t}");
}

#[test]
fn settings_refuse_writes_when_read_only() {
    let home = build_home();
    let mut core = backend(&home);
    core.read_only = true;
    let mut h = Harness::over(home, 160, 48, config(false, ColorMode::TrueColor), core);
    h.keys(&["4", "s"]);
    assert!(
        h.text()
            .contains("✗ cannot write settings.json: read-only mode is enabled"),
        "{}",
        h.text()
    );
}

#[test]
fn stats_overlays_own_their_keys() {
    let mut h = sessions();
    h.keys(&["5", "c"]);
    assert!(h.text().contains("CAVEATS"));
    h.keys(&["d"]);
    assert!(
        h.text().contains("60d ·"),
        "days do not change under the overlay"
    );
    h.keys(&["c"]);
    assert!(!h.text().contains("CAVEATS"));
    h.keys(&["<tab>", "<enter>"]);
    let t = h.text();
    assert!(t.contains("DRILL  --fail"), "{t}");
    assert!(t.contains("open it: peektrace sessions analyze"));
    h.keys(&["a"]);
    assert!(
        h.text().contains("all agents"),
        "agent does not change in the drill"
    );
    h.keys(&["<esc>"]);
    assert!(!h.text().contains("DRILL"));
    h.keys(&["d", "a"]);
    assert!(h.text().contains("90d · Claude"), "{}", h.text());
}

#[test]
fn stats_watch_bump_marks_stale_without_a_rescan() {
    let mut h = sessions();
    h.keys(&["5"]);
    let reports = h.backend.count("Report");
    h.app.watch(WatchVersions {
        memory: 0,
        sessions: 3,
    });
    h.settle();
    assert!(h.text().contains("● stale · R"));
    assert_eq!(h.backend.count("Report"), reports, "a bump never rescans");
    h.keys(&["R"]);
    assert_eq!(h.backend.count("Report"), reports + 1);
    assert!(!h.text().contains("stale"));
}

#[test]
fn stats_below_the_floor_says_so() {
    let mut h = sessions();
    h.keys(&["5"]);
    h.resize(78, 22);
    assert!(
        h.text()
            .contains("terminal too small: stats needs 80×24, this one is 78×22")
    );
}

#[test]
fn token_footer_keeps_the_whole_url() {
    let mut h = Harness::with(build_home(), 60, 20, config(true, ColorMode::TrueColor));
    let t = h.text();
    assert!(t.lines().next().is_some_and(|l| l.contains("token")));
    let joined: String = t
        .lines()
        .rev()
        .skip(1)
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(str::trim)
        .collect();
    assert!(
        joined.contains("0123456789abcdef0123456789abcdef01234567"),
        "{t}"
    );
}

#[test]
fn every_screen_mounts_on_an_empty_home() {
    let mut h = Harness::new(empty_home(), 160, 48);
    assert!(h.text().contains("No sessions found."));
    assert!(h.text().contains("loopback only"));
    h.keys(&["2"]);
    assert!(h.text().contains("No memories found."));
    h.keys(&["3"]);
    assert!(h.text().contains("Capabilities (7)"));
    h.keys(&["4"]);
    assert!(h.text().contains("No extra roots."));
    h.keys(&["5"]);
    assert!(h.text().contains("0 transcripts"), "{}", h.text());
}
