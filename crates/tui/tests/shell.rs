//! The shell on the fixture HOME through the in-process core backend: the
//! header, section switches, each screen's mount, and the server badge and
//! URL footer.

mod common;

use common::*;
use peektrace_tui::theme::ColorMode;

const TOKEN_URL: &str = "http://127.0.0.1:4321/?token=0123456789abcdef0123456789abcdef01234567";
const TOKEN: &str = "0123456789abcdef0123456789abcdef01234567";

fn shell() -> Harness {
    Harness::new(build_home(), 110, 34)
}

fn authed(w: u16, h: u16) -> Harness {
    Harness::with(build_home(), w, h, config(true, ColorMode::TrueColor))
}

#[test]
fn shell_renders_and_switches_to_the_capabilities_matrix() {
    let mut h = shell();
    let first = h.text();
    assert!(first.contains("Peektrace"), "{first}");
    assert!(first.contains("Sessions"));
    assert!(first.contains("web ▸"));
    h.keys(&["3"]);
    let t = h.text();
    assert!(t.contains("supported"), "{t}");
    assert!(t.contains("Capabilities"));
}

#[test]
fn sessions_screen_mounts_its_analysis_pane() {
    let mut h = shell();
    h.keys(&["1"]);
    assert!(h.text().contains("Analysis"), "{}", h.text());
}

#[test]
fn memory_screen_mounts_with_its_web_app_editing_note() {
    let mut h = shell();
    h.keys(&["2"]);
    assert!(
        h.text().contains("editing available in the web app"),
        "{}",
        h.text()
    );
}

#[test]
fn settings_screen_mounts_its_agent_roots_editor() {
    let mut h = shell();
    h.keys(&["4"]);
    assert!(h.text().contains("Agent roots"), "{}", h.text());
}

#[test]
fn loopback_shell_shows_the_loopback_badge() {
    let t = shell().text();
    assert!(t.contains("loopback only"), "{t}");
    assert!(!t.contains("token"), "{t}");
}

#[test]
fn token_shell_shows_the_badge_and_the_full_token_url_at_80_columns() {
    let t = authed(80, 34).text();
    assert!(t.contains("Peektrace"), "{t}");
    assert!(t.contains("web ▸"), "{t}");
    assert!(t.contains(TOKEN_URL), "{t}");
}

#[test]
fn token_footer_keeps_the_key_hints_intact_at_60_columns() {
    let t = authed(60, 20).text();
    let lines: Vec<&str> = t.lines().collect();
    let hints = lines
        .iter()
        .find(|l| l.contains("section"))
        .expect("hint line");
    assert_eq!(hints.trim(), "1-5 section  q quit", "{t}");
    let from = lines
        .iter()
        .position(|l| l.contains("web ▸"))
        .expect("url row");
    let joined: String = lines[from..]
        .iter()
        .filter(|l| !l.contains("section"))
        .map(|l| l.trim())
        .collect::<String>()
        .replace("web▸", "");
    assert!(joined.contains(TOKEN), "{t}");
}
