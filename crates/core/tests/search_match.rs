//! The search matcher and snippet builder (port of
//! `packages/core/test/sessions/search-match.test.ts`). Inputs are copied
//! verbatim; serde sorts object keys, which no case here depends on.

use peektrace_core::export::home::HomeCollapser;
use peektrace_core::search::{
    MatchArgs, Snippet, TranscriptMatch, build_snippet, match_transcript, query_patterns,
    redact_display,
};
use serde_json::{Value, json};

fn lines(rows: &[Value]) -> String {
    rows.iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn match_with(text: &str, q: &str, redact: bool, collapser: &HomeCollapser) -> TranscriptMatch {
    match_transcript(&MatchArgs {
        text,
        q,
        redact,
        max_hits: 3,
        collapser,
    })
}

fn matched(text: &str, q: &str) -> TranscriptMatch {
    match_with(text, q, false, &HomeCollapser::new(None))
}

fn matched_redacted(text: &str, q: &str) -> TranscriptMatch {
    match_with(text, q, true, &HomeCollapser::new(None))
}

fn snippet_of(text: &str, q: &str, redact: bool) -> Snippet {
    let patterns = query_patterns(q);
    build_snippet(text, &patterns.content, redact, &HomeCollapser::new(None))
}

/// `s.slice(start, end)` in UTF-16 units.
fn slice16(s: &str, start: usize, end: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    String::from_utf16_lossy(&units[start..end])
}

fn in_range(s: &Snippet) -> String {
    let (a, b) = s.range.expect("match range");
    slice16(&s.text, a, b)
}

fn first_snippet(m: &TranscriptMatch) -> String {
    m.hits
        .first()
        .map(|h| h.snippet.clone())
        .unwrap_or_default()
}

// matchTranscript

#[test]
fn ignores_key_names_and_malformed_lines() {
    let text = format!("{}\nneedle not json", lines(&[json!({"needle":"x"})]));
    assert_eq!(matched(&text, "needle").match_count, 0);
}

#[test]
fn matches_text_that_json_escapes() {
    let text = lines(&[json!({"text":"line one\nline two"})]);
    assert_eq!(matched(&text, "one\nline").match_count, 1);
}

#[test]
fn ignores_base64_data_and_signatures() {
    let text = lines(&[json!({"data":"QUJDREVGRw==","signature":"QUJDREVGRw=="})]);
    assert_eq!(matched(&text, "QUJDREVG").match_count, 0);
}

#[test]
fn matches_non_ascii_text_case_insensitively() {
    let text = lines(&[json!({"text":"Привет, МИР"})]);
    let r = matched(&text, "привет, мир");
    assert_eq!(r.match_count, 1);
    assert_eq!(first_snippet(&r), "Привет, МИР");
}

#[test]
fn numbers_lines_from_1_and_counts_blank_lines() {
    let text = format!("\n{}", lines(&[json!({"text":"hit"})]));
    assert_eq!(matched(&text, "hit").hits[0].line, 2);
}

// metadata is not searched

#[test]
fn codex_session_meta_git_info_and_turn_context_settings() {
    let text = lines(&[
        json!({"type":"session_meta","payload":{"originator":"codex_cli_rs",
            "git":{"branch":"feature/parsers","repository_url":"git@x:acme/r.git"}}}),
        json!({"type":"turn_context","payload":{"effort":"medium"}}),
    ]);
    assert_eq!(matched(&text, "feature/parsers").match_count, 0);
    assert_eq!(matched(&text, "acme/r").match_count, 0);
    assert_eq!(matched(&text, "medium").match_count, 0);
}

#[test]
fn claude_usage_stop_reason_and_permission_mode() {
    let text = lines(&[
        json!({"type":"assistant","message":{"role":"assistant","stop_reason":"end_turn",
            "usage":{"service_tier":"standard"},"content":[{"type":"text","text":"hi"}]}}),
        json!({"type":"user","permissionMode":"default","message":{"content":"yo"}}),
    ]);
    for q in ["standard", "end_turn", "default"] {
        assert_eq!(matched(&text, q).match_count, 0, "{q}");
    }
    assert_eq!(matched(&text, "hi").match_count, 1);
}

#[test]
fn tool_input_paths_still_match() {
    let text = lines(&[json!({"type":"assistant","message":{"content":[
        {"type":"tool_use","input":{"file_path":"/srv/demo/src/app.ts"}}]}})]);
    assert_eq!(matched(&text, "src/app.ts").match_count, 1);
}

// blank query

#[test]
fn blank_query_matches_nothing() {
    let text = lines(&[json!({"message":{"content":"abc"}})]);
    for q in ["", "   ", "\n\t"] {
        assert_eq!(
            matched(&text, q),
            TranscriptMatch {
                hits: vec![],
                match_count: 0
            },
            "{q:?}"
        );
    }
}

// buildSnippet

fn long_text() -> String {
    format!("{}NEEDLE{}", "a ".repeat(100), " b".repeat(100))
}

#[test]
fn cuts_around_the_match_with_ellipses_and_a_correct_range() {
    let s = snippet_of(&long_text(), "needle", false);
    assert!(s.text.starts_with('…'), "{}", s.text);
    assert!(s.text.ends_with('…'), "{}", s.text);
    assert_eq!(in_range(&s), "NEEDLE");
    assert!(s.text.encode_utf16().count() < 180);
}

#[test]
fn collapses_whitespace() {
    assert_eq!(
        snippet_of("a\n\n  b   NEEDLE", "needle", false).text,
        "a b NEEDLE"
    );
}

#[test]
fn keeps_the_range_when_q_has_repeated_or_odd_whitespace() {
    let text = format!("foo  bar {}", "x".repeat(400));
    for q in ["foo  bar", "foo\tbar", "foo bar"] {
        let s = snippet_of(&text, q, true);
        assert_eq!(in_range(&s), "foo bar", "{q:?}");
    }
}

#[test]
fn stays_on_the_match_when_lowercasing_changes_string_length() {
    let text = format!("{} needle here {}", "İ".repeat(400), "z".repeat(300));
    let r = matched(&lines(&[json!({"message":{"content":text}})]), "needle");
    let hit = &r.hits[0];
    assert!(hit.snippet.contains("needle"), "{}", hit.snippet);
    let (a, b) = (hit.match_start.expect("start"), hit.match_end.expect("end"));
    assert_eq!(slice16(&hit.snippet, a, b), "needle");
}

#[test]
fn strips_terminal_escape_sequences_and_control_characters() {
    let text = "ok \u{1b}[31mERROR\u{1b}[0m \u{1b}]0;pwned\u{7} done\u{0}";
    let s = snippet_of(text, "error", false);
    assert_eq!(s.text, "ok ERROR done");
    assert!(s.text.contains("ERROR"));
    assert!(!s.text.contains("pwned"));
    assert_eq!(in_range(&s), "ERROR");
}

#[test]
fn drops_the_range_when_redaction_removes_the_query() {
    let key = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";
    let s = snippet_of(&format!("token {key} end"), key, true);
    assert_eq!(s.range, None);
    assert!(!s.text.contains(key), "{}", s.text);
}

// queryPatterns

#[test]
fn the_raw_pattern_accepts_the_json_escaped_form() {
    let raw = query_patterns("a\"b c").raw;
    assert!(raw.is_match(r#"{"t":"a\"b c"}"#));
    assert!(raw.is_match(r#"{"t":"a\"b\nc"}"#));
    assert!(!raw.is_match("nothing"));
}

// redaction sees the whole line

fn pem() -> String {
    let body: Vec<String> = (0..20)
        .map(|i| format!("Zm9vYmFyYmF6{i}").repeat(4))
        .collect();
    format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----",
        body.join("\n")
    )
}

#[test]
fn a_long_pem_block_is_hidden_when_the_query_hits_its_middle() {
    let text = lines(&[json!({"message":{"content":format!("{} use it for walrus", pem())}})]);
    for q in ["walrus", "BEGIN PRIVATE", "Zm9vYmFyYmF6"] {
        let snippet = first_snippet(&matched_redacted(&text, q));
        assert!(!snippet.contains("Zm9vYmFyYmF6"), "{q}: {snippet}");
        assert!(!snippet.contains("BEGIN PRIVATE KEY"), "{q}: {snippet}");
    }
}

#[test]
fn a_long_jwt_is_hidden_when_the_query_follows_it() {
    let seg = |n: &str| format!("{:A<160}", format!("eyJ{n}"));
    let jwt = format!("{}.{}.{}", seg("h"), seg("p"), seg("s"));
    let text = lines(&[json!({"message":{"content":format!("token {jwt} walrus")}})]);
    let snippet = first_snippet(&matched_redacted(&text, "walrus"));
    assert!(!snippet.contains("AAAAAAAAAA"), "{snippet}");
    assert!(snippet.contains("walrus"), "{snippet}");
}

// metadata of other agents is not searched

#[test]
fn metadata_of_other_agents_is_not_searched() {
    let cases = [
        (
            "codex phase",
            json!({"type":"response_item","payload":{"phase":"commentary","role":"assistant",
                "content":[{"type":"output_text","text":"hello"}]}}),
            "commentary",
        ),
        (
            "pi provider",
            json!({"type":"model_change","id":"m1","provider":"anthropic","modelId":"claude-x","parentId":null}),
            "anthropic",
        ),
        (
            "pi model id",
            json!({"type":"model_change","id":"m1","provider":"anthropic","modelId":"claude-x"}),
            "claude-x",
        ),
        (
            "pi stop reason",
            json!({"type":"message","message":{"role":"assistant","content":"ok","stopReason":"stop"},"parentId":"pp1"}),
            "stop",
        ),
        (
            "pi parent id",
            json!({"type":"message","message":{"role":"user","content":"ok"},"parentId":"pp1"}),
            "pp1",
        ),
        (
            "opencode directory",
            json!({"kind":"session","directory":"/work/demo","projectId":"prj9"}),
            "demo",
        ),
        (
            "opencode project id",
            json!({"kind":"session","projectId":"prj9"}),
            "prj9",
        ),
        (
            "opencode kind",
            json!({"kind":"message","role":"user"}),
            "message",
        ),
    ];
    for (name, row, q) in cases {
        assert_eq!(matched(&lines(&[row]), q).match_count, 0, "{name}");
    }
}

#[test]
fn codex_response_item_and_event_msg_for_one_message_dedupe() {
    let text = lines(&[
        json!({"type":"response_item","payload":{"phase":"commentary","role":"assistant",
            "content":[{"type":"output_text","text":"Let me run the walrus test."}]}}),
        json!({"type":"event_msg","payload":{"type":"agent_message","message":"Let me run the walrus test."}}),
    ]);
    let r = matched(&text, "walrus");
    let snippets: Vec<&str> = r.hits.iter().map(|h| h.snippet.as_str()).collect();
    assert_eq!(snippets, ["Let me run the walrus test."]);
    assert_eq!(r.match_count, 2);
}

// escaped whitespace beyond newline and tab

#[test]
fn form_feed_and_vertical_tab_separators_match_a_spaced_query() {
    let text = lines(&[json!({"text":"page one\u{c}page two\u{b}end"})]);
    assert_eq!(matched(&text, "one page").match_count, 1);
    assert_eq!(matched(&text, "two end").match_count, 1);
    let tab = lines(&[json!({"text":"tab\tsep\u{c}formfeed walrus3"})]);
    assert_eq!(
        first_snippet(&matched(&tab, "sep formfeed")),
        "tab sep formfeed walrus3"
    );
}

// snippet safety

#[test]
fn bidi_controls_become_spaces() {
    let s = snippet_of("evil \u{202e} txt.exe \u{2066} gnu \u{200f}", "gnu", false);
    assert_eq!(s.text, "evil txt.exe gnu");
}

/// Rust strings cannot hold a lone surrogate; the units layer keeps one as a
/// stand-in char from U+10F800, so a well-formed cut has none of those.
#[test]
fn cuts_never_leave_a_lone_surrogate() {
    let emoji = "\u{1F600}".repeat(100);
    let text = format!("emoji {emoji} walrus5 {emoji}");
    for shift in 0..2 {
        let t = format!("{}{text}", "x".repeat(shift));
        let s = snippet_of(&t, "walrus5", false);
        assert!(
            s.text.chars().all(|c| u32::from(c) < 0x10_F800),
            "shift {shift}: {}",
            s.text
        );
        assert!(s.text.contains("walrus5"));
    }
}

// redaction with a known home

#[test]
fn a_dashed_username_leaves_no_tail() {
    let home = HomeCollapser::new(Some("/home/jane-doe"));
    assert_eq!(redact_display("-home-jane-doe-work", &home), "-~-work");
    assert_eq!(
        redact_display("-home-jane-doe-work", &HomeCollapser::new(None)),
        "-~-doe-work"
    );
    let text = lines(&[json!({"text":"see -home-jane-doe-work needle"})]);
    let hit = first_snippet(&match_with(&text, "needle", true, &home));
    assert!(hit.contains("-~-work"), "{hit}");
    assert!(!hit.contains("doe"), "{hit}");
}
