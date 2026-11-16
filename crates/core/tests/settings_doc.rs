//! The settings document the editor reads (`settings.test.ts`, `parseSettings`).

use peektrace_core::js::json_out::Writer;
use peektrace_core::settings_doc::{settings_doc, write_settings_doc};

fn doc(raw: &str) -> String {
    let mut w = Writer::new(false);
    write_settings_doc(&mut w, &settings_doc(raw));
    w.finish()
}

#[test]
fn empty_or_blank_text_yields_the_empty_config() {
    assert_eq!(doc(""), "{}");
    assert_eq!(doc("   \n "), "{}");
}

#[test]
fn malformed_json_falls_back_to_the_empty_config() {
    assert_eq!(doc("{ not json"), "{}");
}

#[test]
fn parses_per_agent_extra_roots_with_optional_labels() {
    assert_eq!(
        doc(
            r#"{"roots":{"claude":[{"path":"~/work/.claude","label":"work"},{"path":"/abs/.claude"}]}}"#
        ),
        r#"{"roots":{"claude":[{"path":"~/work/.claude","label":"work"},{"path":"/abs/.claude"}]}}"#
    );
}

#[test]
fn ignores_an_unknown_agent_key() {
    assert_eq!(doc(r#"{"roots":{"bogus":[]}}"#), r#"{"roots":{}}"#);
}

#[test]
fn rejects_a_root_entry_missing_path() {
    assert_eq!(doc(r#"{"roots":{"claude":[{"label":"x"}]}}"#), "{}");
}

#[test]
fn salvages_a_valid_agent_when_another_agents_value_is_malformed() {
    assert_eq!(
        doc(r#"{"roots":{"claude":[{"path":"/a"}],"codex":"not-an-array"}}"#),
        r#"{"roots":{"claude":[{"path":"/a"}]}}"#
    );
}

#[test]
fn drops_a_single_bad_entry_but_keeps_valid_ones_in_the_same_agent() {
    assert_eq!(
        doc(r#"{"roots":{"claude":[{"path":"/good"},{"bad":true},{"path":"/ok"}]}}"#),
        r#"{"roots":{"claude":[{"path":"/good"},{"path":"/ok"}]}}"#
    );
}
