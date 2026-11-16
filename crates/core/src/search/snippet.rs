//! Printable snippets around a match: redacted, stripped of terminal
//! escapes and control chars, cut to 80 UTF-16 units each side.

use std::borrow::Cow;

use crate::export::home::HomeCollapser;
use crate::js::regex::{LazyRe, Re};
use crate::js::{text, units};
use crate::js_re;
use crate::sessions::redact::redact_text;

const CONTEXT: usize = 80;

static ANSI_OSC: LazyRe = js_re!(r"\u001b\][^\u0007\u001b]*(?:\u0007|\u001b\\)?", "g");
static ANSI_CSI: LazyRe = js_re!(r"\u001b\[[0-9;?]*[ -/]*[@-~]?", "g");
static ANSI_ESC: LazyRe = js_re!(r"\u001b.?", "g");
/// C0, DEL, C1 and the bidi marks that can disguise text when printed.
static CONTROL: LazyRe = js_re!(
    r"[\u0000-\u001f\u007f-\u009f\u200e\u200f\u202a-\u202e\u2066-\u2069]",
    "g"
);

/// A snippet and the UTF-16 range of the match in it, when it survived
/// redaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    pub text: String,
    pub range: Option<(usize, usize)>,
}

/// `collapsePaths(redactText(s))`.
pub fn redact_display(s: &str, collapser: &HomeCollapser) -> String {
    collapser.collapse(&redact_text(s))
}

/// Drop terminal escape sequences and fold whitespace.
fn collapse(s: &str) -> String {
    let s = [&*ANSI_OSC, &*ANSI_CSI, &*ANSI_ESC, &*CONTROL]
        .into_iter()
        .fold(Cow::Borrowed(s), |s, re| {
            if re.is_match(&s) {
                Cow::Owned(re.replace_all_str(&s, " "))
            } else {
                s
            }
        });
    text::trim(&units::collapse_ws(&s)).to_owned()
}

fn is_high(u: u16) -> bool {
    (0xD800..0xDC00).contains(&u)
}

fn is_low(u: u16) -> bool {
    (0xDC00..0xE000).contains(&u)
}

/// `CONTEXT` units each side of `[at, at + len)` with ellipses, and where
/// the match starts in the result.
fn cut(s: &[u16], at: usize, len: usize) -> (String, usize) {
    let mut from = at.saturating_sub(CONTEXT);
    let mut to = s.len().min(at + len + CONTEXT);
    if from > 0 && s.get(from).copied().is_some_and(is_low) {
        from += 1;
    }
    if to < s.len() && to > 0 && is_high(s[to - 1]) {
        to -= 1;
    }
    let lead = if from > 0 { "…" } else { "" };
    let tail = if to < s.len() { "…" } else { "" };
    let body = units::from_units(&s[from.min(to)..to]);
    let start = at + usize::from(from > 0) - from;
    (format!("{lead}{body}{tail}"), start)
}

/// Snippet around the first match of `pattern` in `text`.
pub fn build_snippet(text: &str, pattern: &Re, redact: bool, collapser: &HomeCollapser) -> Snippet {
    let window = if redact {
        collapse(&redact_display(text, collapser))
    } else {
        collapse(text)
    };
    let all = units::to_units(&window);
    match pattern.find(&window) {
        None => Snippet {
            text: cut(&all, all.len() / 2, 0).0,
            range: None,
        },
        Some(r) => {
            let at = units::len(&window[..r.start]);
            let len = units::len(&window[r]);
            let (text, start) = cut(&all, at, len);
            Snippet {
                text,
                range: Some((start, start + len)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_controls() {
        assert_eq!(
            collapse("ok \u{1b}[31mERROR\u{1b}[0m \u{1b}]0;pwned\u{7} done\u{0}"),
            "ok ERROR done"
        );
        assert_eq!(
            collapse("evil \u{202e} txt.exe \u{2066} gnu \u{200f}"),
            "evil txt.exe gnu"
        );
    }

    #[test]
    fn cuts_keep_pairs_whole() {
        let s = units::to_units(&format!("{}walrus{}", "x".repeat(300), "y".repeat(300)));
        let (text, start) = cut(&s, 300, 6);
        assert_eq!(start, 81);
        assert_eq!(
            text,
            format!("…{}walrus{}…", "x".repeat(80), "y".repeat(80))
        );
        let e = units::to_units(&format!("a{}walrus", "😀".repeat(50)));
        let (text, _) = cut(&e, units::len(&format!("a{}", "😀".repeat(50))), 6);
        assert!(text.starts_with("…😀"));
    }
}
