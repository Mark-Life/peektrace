//! Highlighted bodies: the shared tokenizer's classes painted GitHub-dark,
//! capped before tokenizing, then word-wrapped into rows.

use peektrace_core::highlight::{
    CodeLine, Lang, TokenClass, is_highlightable, plain_lines, tokenize_lines,
};
use peektrace_core::js::{text, units};
use ratatui::style::{Color, Modifier, Style};

use crate::draw::{Seg, wrap};
use crate::fmt::sanitize;
use crate::theme;

/// Longest line kept before eliding, in UTF-16 units.
const MAX_LINE_CHARS: usize = 1000;
/// Characters kept per body.
const MAX_BODY_CHARS: usize = 20_000;

pub fn style_of(class: Option<TokenClass>) -> Style {
    let c = |hex: u32| {
        #[expect(clippy::cast_possible_truncation, reason = "byte extraction")]
        Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    };
    let Some(class) = class else {
        return theme::fg(theme::TEXT);
    };
    let (hex, modifier) = match class {
        TokenClass::Keyword => (0xff_7b72, Modifier::BOLD),
        TokenClass::Operator => (0xff_7b72, Modifier::empty()),
        TokenClass::String | TokenClass::CodeInline => (0xa5_d6ff, Modifier::empty()),
        TokenClass::Number | TokenClass::Literal | TokenClass::Attr | TokenClass::Property => {
            (0x79_c0ff, Modifier::empty())
        }
        TokenClass::Function | TokenClass::Command => (0xd2_a8ff, Modifier::empty()),
        TokenClass::Type => (0xff_a657, Modifier::empty()),
        TokenClass::Tag | TokenClass::Selector | TokenClass::Inserted => {
            (0x7e_e787, Modifier::empty())
        }
        TokenClass::Deleted => (0xff_a198, Modifier::empty()),
        TokenClass::Comment => (0x8b_949e, Modifier::ITALIC),
        TokenClass::Meta => (0x8b_949e, Modifier::empty()),
        TokenClass::Heading => (0x58_a6ff, Modifier::BOLD),
        TokenClass::Link => (0x58_a6ff, Modifier::UNDERLINED),
        TokenClass::Variable => (0xe6_edf3, Modifier::empty()),
    };
    Style::new().fg(c(hex)).add_modifier(modifier)
}

fn cap_line(line: &str) -> String {
    let n = units::len(line);
    if n <= MAX_LINE_CHARS {
        line.to_owned()
    } else {
        format!(
            "{}… +{} more chars",
            units::slice(line, 0, MAX_LINE_CHARS),
            n - MAX_LINE_CHARS
        )
    }
}

/// Bound a body before it reaches the tokenizer: `max_lines` lines, 1,000
/// chars per line and 20,000 chars overall.
pub fn cap_lines(s: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = s.split('\n').collect();
    let mut kept: Vec<String> = Vec::new();
    let mut budget = isize::try_from(MAX_BODY_CHARS).unwrap_or(isize::MAX);
    for line in &lines {
        if kept.len() >= max_lines || budget <= 0 {
            break;
        }
        let capped = cap_line(line);
        budget -= isize::try_from(units::len(&capped) + 1).unwrap_or(isize::MAX);
        kept.push(capped);
    }
    if kept.len() < lines.len() {
        kept.push(format!("… +{} more lines", lines.len() - kept.len()));
    }
    kept.join("\n")
}

/// Merge tokenized lines into one run of segments, one per style change.
fn styled(lines: &[CodeLine<'_>]) -> Vec<Seg> {
    let mut out: Vec<Seg> = Vec::new();
    let mut push = |t: &str, s: Style| match out.last_mut() {
        Some((text, st)) if *st == s => text.push_str(t),
        _ => out.push((t.to_owned(), s)),
    };
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            push("\n", style_of(None));
        }
        for t in line {
            push(t.value, style_of(t.class));
        }
    }
    out
}

/// `content` sanitized, capped, highlighted for `lang` (`None` is plain)
/// and wrapped to `w` columns.
pub fn highlighted(content: &str, lang: Option<Lang>, max_lines: usize, w: usize) -> Vec<Vec<Seg>> {
    let clean = cap_lines(&sanitize(content), max_lines);
    match lang {
        Some(lang) if !text::trim(&clean).is_empty() => {
            let lines = if is_highlightable(&clean, lang) {
                tokenize_lines(&clean, lang)
            } else {
                plain_lines(&clean)
            };
            wrap(&styled(&lines), w)
        }
        _ => {
            let shown = if clean.is_empty() {
                "—"
            } else {
                clean.as_str()
            };
            wrap(&[(shown.to_owned(), theme::fg(theme::TEXT))], w)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_lines_and_chars() {
        let body = (0..130)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let capped = cap_lines(&body, 120);
        assert!(capped.ends_with("… +10 more lines"), "{capped}");
        let long = "x".repeat(1005);
        assert_eq!(
            cap_lines(&long, 10),
            format!("{}… +5 more chars", "x".repeat(1000))
        );
    }

    #[test]
    fn strips_ansi_before_tokenizing() {
        let rows = highlighted("\u{1b}[31mls\u{1b}[0m -la", Some(Lang::Bash), 10, 40);
        let text: String = rows[0].iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(text, "ls -la");
        assert_eq!(rows[0][0].1, style_of(Some(TokenClass::Command)));
    }
}
