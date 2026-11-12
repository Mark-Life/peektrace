//! The shared tokenizer behind web code blocks and TUI bodies.

use std::time::Instant;

use peektrace_core::highlight::{CodeLine, Lang, is_highlightable, plain_lines, tokenize_lines};

fn shape(lines: &[CodeLine<'_>]) -> Vec<Vec<String>> {
    lines
        .iter()
        .map(|l| {
            l.iter()
                .map(|t| match t.class {
                    Some(c) => format!("{}:{}", c.as_str(), t.value),
                    None => t.value.to_owned(),
                })
                .collect()
        })
        .collect()
}

fn flat(code: &str, lang: Lang) -> Vec<String> {
    shape(&tokenize_lines(code, lang)).concat()
}

#[test]
fn classifies_typescript_tokens() {
    assert_eq!(
        shape(&tokenize_lines(
            "const x: number = 1 // hi",
            Lang::Typescript
        )),
        vec![vec![
            "keyword:const",
            " x: ",
            "type:number",
            " = ",
            "number:1",
            " ",
            "comment:// hi"
        ]]
    );
}

#[test]
fn splits_multi_line_tokens_into_one_entry_per_line() {
    assert_eq!(
        shape(&tokenize_lines("/* a\nb */\nlet y", Lang::Typescript)),
        vec![
            vec!["comment:/* a"],
            vec!["comment:b */"],
            vec!["keyword:let", " y"]
        ]
    );
}

#[test]
fn keeps_empty_lines() {
    assert_eq!(tokenize_lines("{\n\n  \"a\": 1\n}", Lang::Json).len(), 4);
}

#[test]
fn tokenizes_bash_json_and_markdown() {
    assert!(flat("echo \"hi\"", Lang::Bash).contains(&"string:\"hi\"".to_owned()));
    assert!(flat("{\"a\": 1}", Lang::Json).contains(&"number:1".to_owned()));
    assert!(flat("# Title", Lang::Markdown).contains(&"heading:# Title".to_owned()));
}

#[test]
fn leaves_text_unclassified() {
    assert_eq!(
        shape(&tokenize_lines("const x", Lang::Text)),
        vec![vec!["const x"]]
    );
}

#[test]
fn plain_lines_emit_one_classless_token_per_non_empty_line() {
    let want: Vec<Vec<String>> = vec![vec!["a".into()], vec![], vec!["b".into()]];
    assert_eq!(shape(&plain_lines("a\n\nb")), want);
}

#[test]
fn caps_linear_tokenizers_at_1_mb_and_bash_at_20_kb() {
    assert!(is_highlightable(&"x".repeat(1_000_000), Lang::Json));
    assert!(!is_highlightable(&"x".repeat(1_000_001), Lang::Typescript));
    assert!(is_highlightable(&"x".repeat(20_000), Lang::Bash));
    assert!(!is_highlightable(&"x".repeat(20_001), Lang::Bash));
}

#[test]
fn bash_at_the_cap_stays_fast_on_heredoc_heavy_input() {
    let code: String = "cat <<'EOF'\na << b\nEOF\n".repeat(1000)[..20_000].to_owned();
    assert!(is_highlightable(&code, Lang::Bash));
    let start = Instant::now();
    tokenize_lines(&code, Lang::Bash);
    assert!(start.elapsed().as_millis() < 2000);
}

#[test]
fn templates_fences_and_heredocs() {
    assert_eq!(
        flat("const s = `a${b}c`", Lang::Typescript),
        vec![
            "keyword:const",
            " s = ",
            "string:`a",
            "operator:${",
            "b",
            "operator:}",
            "string:c`"
        ]
    );
    let md = flat("```bash\necho hi\n```", Lang::Markdown);
    assert_eq!(
        md,
        vec![
            "meta:```bash",
            "command:echo",
            "code-inline: hi",
            "meta:```"
        ]
    );
    let sh = flat("cat <<EOF\nbody\nEOF\nls", Lang::Bash);
    assert_eq!(
        sh,
        vec![
            "command:cat",
            " <<EOF",
            "string:body",
            "meta:EOF",
            "command:ls"
        ]
    );
}
