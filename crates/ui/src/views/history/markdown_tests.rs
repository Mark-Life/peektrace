//! Ports of `apps/inspector/test/markdown.test.tsx`: the transcript Markdown
//! body against hostile input, rendered through Rune's `Runtime`.

use std::cell::RefCell;

use rune::{Cx, El, ElementInfo, Env, Runtime, col};

use super::body::{MARKDOWN_CHAR_LIMIT, markdown};
use crate::state::Ui;
use crate::theme::theme;

thread_local! {
    static SRC: RefCell<String> = const { RefCell::new(String::new()) };
}

fn view(_s: &Ui, _cx: &mut Cx<Ui>) -> El {
    let src = SRC.with(|s| s.borrow().clone());
    col().w(900.0).padding(10.0).child(markdown(&src, &theme(true)))
}

fn render(src: &str) -> Runtime<Ui> {
    SRC.with(|s| *s.borrow_mut() = src.to_owned());
    let mut rt = Runtime::new(Ui::new(None, 0.0, true), view, (1000.0, 800.0), Env::new(true, 1.0));
    for _ in 0..4 {
        rt.frame(1.0 / 60.0);
    }
    rt
}

fn texts(rt: &Runtime<Ui>) -> String {
    rt.elements()
        .into_iter()
        .filter_map(|e| e.text.or(e.label))
        .collect::<Vec<_>>()
        .join("\n")
}

fn with_role(rt: &Runtime<Ui>, role: &str) -> Vec<ElementInfo> {
    rt.elements().into_iter().filter(|e| e.role == role).collect()
}

/// Press the middle of the first link (else the first text) and return
/// the links opened.
fn press_link(src: &str) -> Vec<String> {
    let mut rt = render(src);
    let els = rt.elements();
    let Some(e) = els
        .iter()
        .find(|e| e.role == "link")
        .or_else(|| els.iter().find(|e| e.text.is_some()))
    else {
        panic!("no text for {src}");
    };
    let [x, y, w, h] = e.rect;
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    rt.pointer_move(cx, cy);
    rt.pointer_down(cx, cy);
    rt.pointer_up(cx, cy);
    rt.frame(1.0 / 60.0);
    rt.take_url_requests()
}

#[test]
fn raw_html_is_shown_as_text_never_injected() {
    let rt = render(
        "<script>alert(1)</script>\n\n<img src=x onerror=\"alert(1)\">\n\nhi <b onclick=\"x\">b</b>",
    );
    let all = texts(&rt);
    assert!(all.contains("<script>alert(1)</script>"), "{all}");
    assert!(all.contains("<b onclick="), "{all}");
    assert!(with_role(&rt, "image").is_empty());
}

#[test]
fn tag_like_harness_markers_stay_readable_text() {
    let all = texts(&render("<system-reminder>\nbe brief\n</system-reminder>"));
    assert!(all.contains("<system-reminder>"), "{all}");
}

#[test]
fn executable_and_data_urls_open_nothing_but_keep_the_label() {
    for href in [
        "javascript:alert(1)",
        "JaVaScRiPt:alert(1)",
        "java\tscript:alert(1)",
        "data:text/html,<b>x</b>",
        "vbscript:msgbox(1)",
    ] {
        let src = format!("[click]({href})");
        assert!(texts(&render(&src)).contains("click"), "{href}");
        assert_eq!(press_link(&src), Vec::<String>::new(), "{href}");
    }
}

#[test]
fn relative_and_in_page_links_do_not_navigate() {
    assert_eq!(press_link("[a](#/settings)"), Vec::<String>::new());
    assert_eq!(press_link("[b](./x.md)"), Vec::<String>::new());
}

#[test]
fn web_links_open_in_the_system_browser() {
    assert_eq!(
        press_link("[docs](https://example.com/a?b=1)"),
        ["https://example.com/a?b=1"]
    );
}

#[test]
fn images_show_their_alt_text_and_fetch_nothing() {
    let rt = render("![chart](https://example.com/c.png)");
    let images = with_role(&rt, "image");
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].label.as_deref(), Some("chart"));
    assert!(texts(&rt).contains("chart"));
}

#[test]
fn a_fenced_block_renders_as_code() {
    let all = texts(&render("Run:\n\n```ts\nconst a: number = 1;\n```"));
    assert!(all.contains("Run:"), "{all}");
    assert!(all.contains("const a: number = 1;"), "{all}");
}

#[test]
fn fences_nested_in_lists_and_quotes_render_as_code() {
    let all = texts(&render(
        "- step\n\n  ```bash\n  ls\n  ```\n\n> ```json\n> {}\n> ```",
    ));
    assert!(all.lines().any(|l| l.trim() == "ls"), "{all}");
    assert!(all.lines().any(|l| l.trim() == "{}"), "{all}");
    assert!(!all.contains("```"), "{all}");
}

#[test]
fn lists_emphasis_and_inline_code_render_as_elements() {
    let rt = render("- **bold** and `x`\n- two");
    let all = texts(&rt);
    assert!(!all.contains("**"), "{all}");
    assert!(!all.contains('`'), "{all}");
    assert!(all.contains("bold"), "{all}");
    assert!(all.contains("two"), "{all}");
}

#[test]
fn headings_carry_no_key_taken_from_their_text() {
    let rt = render("# Title");
    let headings = with_role(&rt, "heading");
    assert_eq!(headings.len(), 1);
    // The kit keys blocks by position (`b0`), never by heading text.
    assert_eq!(headings[0].key.as_deref(), Some("b0"));
}

#[test]
fn frontmatter_stays_visible() {
    let rt = render("---\nname: skill\n---\n\n# Body");
    assert!(texts(&rt).contains("name: skill"));
    assert!(
        with_role(&rt, "heading")
            .iter()
            .any(|e| e.text.as_deref() == Some("Body") || e.label.as_deref() == Some("Body"))
    );
}

#[test]
fn text_past_the_size_limit_renders_verbatim() {
    let big = format!("# heading\n{}", "x".repeat(MARKDOWN_CHAR_LIMIT));
    let rt = render(&big);
    assert!(with_role(&rt, "heading").is_empty());
    assert!(texts(&rt).contains("# heading"));
}

