//! Buffer snapshots of each screen and state on the fixture HOME, at the
//! sizes where the TS TUI broke. `UPDATE_SNAPSHOTS=1` rewrites them.

mod common;

use common::*;
use peektrace_tui::theme::ColorMode;

fn at(w: u16, h: u16) -> Harness {
    Harness::new(build_home(), w, h)
}

#[test]
fn sessions() {
    snap_styled(&mut at(160, 48), "sessions-160x48");
    snap(&mut at(120, 36), "sessions-120x36");
    snap(&mut at(100, 30), "sessions-100x30");
    let mut h = at(160, 48);
    h.keys(&["/", "f", "l", "a"]);
    snap(&mut h, "sessions-filter-typing");
    h.keys(&["<esc>", "a", "a"]);
    snap(&mut h, "sessions-facet-codex");
    snap(&mut Harness::new(empty_home(), 160, 48), "sessions-empty");
}

#[test]
fn history() {
    let mut h = at(160, 48);
    h.keys(&["<enter>", "j", "j", "j", "<enter>", "j", "<enter>"]);
    snap_styled(&mut h, "history-bash-open");
    h.keys(&["<home>", "E"]);
    snap(&mut h, "history-expand-all");
    h.keys(&["C", "s"]);
    snap(&mut h, "history-biggest-first");
    let mut h = at(100, 30);
    h.keys(&["<enter>", "j", "j", "<enter>"]);
    snap(&mut h, "history-100x30");
    let mut h = at(120, 36);
    h.keys(&["<enter>", "j", "j", "j", "j", "j", "<enter>"]);
    snap(&mut h, "history-120x36-markdown");
}

#[test]
fn growth() {
    let mut h = at(160, 48);
    h.keys(&["g"]);
    snap_styled(&mut h, "growth-160x48");
    let mut h = at(100, 30);
    h.keys(&["g"]);
    snap(&mut h, "growth-100x30");
}

#[test]
fn memory() {
    let mut h = at(160, 48);
    h.keys(&["2"]);
    snap_styled(&mut h, "memory-vault");
    h.keys(&["j", "j", "j"]);
    snap(&mut h, "memory-entry");
    let mut h = at(100, 30);
    h.keys(&["2", "j", "j", "j"]);
    snap(&mut h, "memory-100x30");
}

#[test]
fn capabilities() {
    let mut h = at(160, 48);
    h.keys(&["3", "j"]);
    snap_styled(&mut h, "capabilities-moved");
    let mut h = at(100, 30);
    h.keys(&["3"]);
    snap(&mut h, "capabilities-100x30");
}

#[test]
fn settings() {
    let mut h = at(160, 48);
    h.keys(&["4"]);
    snap_styled(&mut h, "settings");
    h.keys(&["<enter>"]);
    snap(&mut h, "settings-editing");
    let mut h = at(100, 30);
    h.keys(&["4"]);
    snap(&mut h, "settings-100x30");
}

#[test]
fn stats() {
    let mut h = at(160, 48);
    h.keys(&["5"]);
    snap_styled(&mut h, "stats-160x48");
    h.keys(&["<tab>", "<enter>"]);
    snap(&mut h, "stats-drill");
    h.keys(&["<esc>", "c"]);
    snap(&mut h, "stats-caveats");
    let mut h = at(100, 30);
    h.keys(&["5"]);
    snap(&mut h, "stats-100x30");
    let mut h = at(80, 24);
    h.keys(&["5"]);
    snap(&mut h, "stats-80x24");
    let mut h = at(78, 22);
    h.keys(&["5"]);
    snap(&mut h, "stats-78x22");
}

#[test]
fn shell() {
    snap(
        &mut Harness::with(build_home(), 60, 20, config(true, ColorMode::TrueColor)),
        "shell-token-60x20",
    );
    snap(
        &mut Harness::with(build_home(), 80, 24, config(true, ColorMode::TrueColor)),
        "shell-token-80x24",
    );
}

#[test]
fn every_screen_draws_at_every_size_without_panicking() {
    for authed in [false, true] {
        let mut h = Harness::with(build_home(), 160, 48, config(authed, ColorMode::Ansi256));
        h.keys(&["<enter>", "E", "<tab>"]);
        for section in ["1", "2", "3", "4", "5"] {
            h.keys(&[section]);
            if section == "5" {
                h.keys(&["<tab>", "<enter>"]);
            }
            for (w, ht) in [
                (1, 1),
                (3, 2),
                (10, 5),
                (20, 8),
                (40, 12),
                (60, 20),
                (78, 22),
                (80, 24),
                (100, 30),
                (200, 60),
            ] {
                h.resize(w, ht);
                h.draw();
            }
            h.resize(160, 48);
            h.keys(&["<esc>"]);
        }
    }
}
