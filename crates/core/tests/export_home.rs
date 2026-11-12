//! Home-path collapsing in exports. Ported from the `collapseHomePaths`
//! cases of the `export` TS test.

use peektrace_core::export::{ExportOptions, HomeCollapser, render_json};
use peektrace_core::sessions::model::{ParsedSession, Provider};

fn c(text: &str) -> String {
    HomeCollapser::new(None).collapse(text)
}

fn ch(text: &str, home: &str) -> String {
    HomeCollapser::new(Some(home)).collapse(text)
}

const UNIX: &str = "/home/jane-doe";
const WIN: &str = r"C:\Users\jane-doe";

#[test]
fn leaves_cli_flags_that_look_like_home_slugs_alone() {
    for text in [
        "pip install --home-page=x",
        "gpg --home-dir ./g",
        "tool --users-list --users-file a.csv",
        "flag --Users-only",
        "and -users-x",
    ] {
        assert_eq!(c(text), text);
        assert_eq!(ch(text, "/home/jane-doe"), text);
    }
}

#[test]
fn collapses_home_prefixes_on_both_oses() {
    assert_eq!(c("/home/x/a.ts"), "~/a.ts");
    assert_eq!(c("/Users/x"), "~");
    assert_eq!(c(r"C:\Users\x\a.ts"), r"~\a.ts");
    assert_eq!(c(r"C:\\Users\\x\\a"), r"~\\a");
}

#[test]
fn collapses_uri_wsl_and_slug_forms() {
    assert_eq!(
        c("open file:///home/alice/notes.txt"),
        "open file:///~/notes.txt"
    );
    assert_eq!(c("vscode://file/home/alice/x"), "vscode://file/~/x");
    assert_eq!(c("file:///Users/alice/x"), "file:///~/x");
    assert_eq!(c("/mnt/c/Users/bob/x"), "~/x");
    assert_eq!(c("/c/Users/bob/x"), "~/x");
    assert_eq!(
        c("/home/alice/.claude/projects/-home-alice-Code-proj/memory"),
        "~/.claude/projects/-~-Code-proj/memory"
    );
    assert_eq!(c("C--Users-bob-proj"), "-~-proj");
}

#[test]
fn keeps_surrounding_shell_and_bracket_text() {
    assert_eq!(c("echo ${HOME:-/home/alice}"), "echo ${HOME:-~}");
    assert_eq!(c("see [/home/alice] ok"), "see [~] ok");
    assert_eq!(c("cd /home/alice&&ls"), "cd ~&&ls");
    assert_eq!(c("in /home/alice."), "in ~.");
}

#[test]
fn handles_spaced_names_and_windows_letter_case() {
    assert_eq!(c("/Users/John Smith/proj"), "~/proj");
    assert_eq!(c(r"C:\Users\John Smith\proj"), r"~\proj");
    assert_eq!(c(r"c:\users\bob\x"), r"~\x");
}

#[test]
fn collapses_mid_string_and_leaves_lookalikes() {
    assert_eq!(
        c(r#"{"p":"/home/x/a"} and /Users/y/b"#),
        r#"{"p":"~/a"} and ~/b"#
    );
    assert_eq!(c("/homework/a /opt/home/x/a"), "/homework/a /opt/home/x/a");
}

#[test]
fn without_a_home_a_dashed_username_leaks_in_slugs_only() {
    assert_eq!(c("-home-jane-doe-work"), "-~-doe-work");
    assert_eq!(c("C--Users-jane-doe-work"), "-~-doe-work");
    assert_eq!(c("/home/jane-doe/x"), "~/x");
    assert_eq!(c(r"C:\Users\jane-doe\x"), r"~\x");
}

#[test]
fn collapses_the_unix_home_and_its_slug_exactly() {
    assert_eq!(ch("-home-jane-doe-work", UNIX), "-~-work");
    assert_eq!(ch("-home-jane-doe", UNIX), "-~");
    assert_eq!(ch("/home/jane-doe/x", UNIX), "~/x");
    assert_eq!(ch("file:///home/jane-doe/x", UNIX), "file:///~/x");
}

#[test]
fn collapses_the_windows_home_and_its_slug_exactly() {
    assert_eq!(ch("C--Users-jane-doe-work", WIN), "-~-work");
    assert_eq!(ch(r"C:\Users\jane-doe\x", WIN), r"~\x");
    assert_eq!(ch("C:/Users/jane-doe/x", WIN), "~/x");
    assert_eq!(ch(r"c:\users\jane-doe\x", WIN), r"~\x");
}

#[test]
fn matches_windows_homes_and_slugs_in_any_case() {
    let home = r"C:\Users\Jane-Doe";
    assert_eq!(ch("C--Users-jane-doe-work", home), "-~-work");
    assert_eq!(ch("c--users-jane-doe-work", home), "-~-work");
    assert_eq!(c("c--Users-jane-doe-work"), "-~-doe-work");
}

#[test]
fn collapses_pi_slugs() {
    assert_eq!(ch("--home-jane-doe-work--", UNIX), "--~-work--");
    assert_eq!(ch("--home-jane-doe-.cache-x--", UNIX), "--~-.cache-x--");
    assert_eq!(ch("--home-jane-doer-x--", UNIX), "--~-doer-x--");
    assert_eq!(
        ch("--C--Users-Jane-Doe-work--", r"C:\Users\Jane-Doe"),
        "--~-work--"
    );
    assert_eq!(c("--home-jane-work--"), "--~-work--");
    assert_eq!(c("--home-jane-doe-work--"), "--~-doe-work--");
    assert_eq!(c("--Users-jane-work--"), "--~-work--");
    assert_eq!(c("--C--Users-jane-work--"), "--~-work--");
}

#[test]
fn keeps_three_slashes_in_file_uris_with_a_windows_home() {
    assert_eq!(ch("file:///C:/Users/jane-doe/x", WIN), "file:///~/x");
}

#[test]
fn leaves_no_part_of_the_username() {
    let out = ch("-home-jane-doe-work /home/jane-doe/a", UNIX);
    assert!(!out.contains("doe"), "{out}");
    assert!(!out.contains("jane"), "{out}");
}

#[test]
fn does_not_touch_longer_names_or_a_trailing_separator_home() {
    assert_eq!(ch("/home/jane-doer/x", UNIX), "~/x");
    assert_eq!(ch("-home-jane-doe-work", &format!("{UNIX}/")), "-~-work");
}

#[test]
fn renders_exports_with_the_home() {
    let session = ParsedSession {
        provider: Provider::ClaudeCode,
        session_id: "s".to_owned(),
        path: String::new(),
        models: Vec::new(),
        events: Vec::new(),
        turns: Vec::new(),
        compaction_indexes: Vec::new(),
        subagents: Vec::new(),
        cwd: None,
        git_branch: None,
        title: Some("-home-jane-doe-work".to_owned()),
        version: None,
        started_at: None,
        ended_at: None,
        native_context_window: None,
        unknown_record_types: None,
        unknown_system_subtypes: None,
        sidechain_cost: None,
        native_cost_usd: None,
    };
    let out = render_json(
        &session,
        &ExportOptions {
            exported_at: "2026-01-01T00:00:00Z".to_owned(),
            home: Some(UNIX.to_owned()),
            redacted: true,
        },
    );
    assert!(!out.contains("doe"), "{out}");
}

#[test]
fn var_home_and_wsl_unc_paths() {
    assert_eq!(c("/var/home/alice/x"), "~/x");
    assert_eq!(c(r"\\wsl.localhost\Ubuntu\home\alice\x"), r"~\x");
    assert_eq!(c(r"\\wsl$\Ubuntu\home\alice\x"), r"~\x");
}

#[test]
fn non_ascii_user_names_collapse_in_full() {
    assert_eq!(c("/Users/\u{D3}lafur/x"), "~/x");
    assert_eq!(c("C:\\Users\\\u{C1}d\u{E1}m\\x"), r"~\x");
    assert_eq!(c("/home/\u{438}\u{432}\u{430}\u{43D}/x"), "~/x");
    assert_eq!(c("/Users/Jos\u{E9}/x"), "~/x");
    assert_eq!(c("C:\\Users\\Zo\u{EB}\\proj"), r"~\proj");
    assert_eq!(c("-Users-Jos\u{E9}-proj"), "-~-proj");
}

#[test]
fn names_with_three_or_more_words() {
    assert_eq!(c("/Users/John Smith Jr/x"), "~/x");
}
