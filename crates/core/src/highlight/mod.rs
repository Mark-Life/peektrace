//! Syntax tokenizer shared by the terminal UI and the web UI: a port of the
//! `@tanstack/highlight` 1.1.0 languages TS registers (`ts`, `shell`, `json`,
//! `markdown`, `plaintext`). Output is lines of `(class, text)` tokens; each
//! surface maps the class onto its own palette.

mod markup;
mod patterns;
mod script;
mod shell;

use crate::js::{text, units};

/// The class names `@tanstack/highlight` emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenClass {
    Attr,
    CodeInline,
    Command,
    Comment,
    Deleted,
    Function,
    Heading,
    Inserted,
    Keyword,
    Link,
    Literal,
    Meta,
    Number,
    Operator,
    Property,
    Selector,
    String,
    Tag,
    Type,
    Variable,
}

impl TokenClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Attr => "attr",
            Self::CodeInline => "code-inline",
            Self::Command => "command",
            Self::Comment => "comment",
            Self::Deleted => "deleted",
            Self::Function => "function",
            Self::Heading => "heading",
            Self::Inserted => "inserted",
            Self::Keyword => "keyword",
            Self::Link => "link",
            Self::Literal => "literal",
            Self::Meta => "meta",
            Self::Number => "number",
            Self::Operator => "operator",
            Self::Property => "property",
            Self::Selector => "selector",
            Self::String => "string",
            Self::Tag => "tag",
            Self::Type => "type",
            Self::Variable => "variable",
        }
    }
}

/// Languages the tokenizer is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Typescript,
    Bash,
    Json,
    Markdown,
    Text,
}

impl Lang {
    pub const ALL: [Self; 5] = [
        Self::Typescript,
        Self::Bash,
        Self::Json,
        Self::Markdown,
        Self::Text,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Typescript => "typescript",
            Self::Bash => "bash",
            Self::Json => "json",
            Self::Markdown => "markdown",
            Self::Text => "text",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.as_str() == s)
    }

    /// Largest input, in UTF-16 units, tokenized without blocking a frame.
    /// The shell tokenizer is quadratic in heredocs.
    const fn limit(self) -> usize {
        match self {
            Self::Bash => 20_000,
            _ => 1_000_000,
        }
    }

    const fn def(self) -> Def {
        match self {
            Self::Typescript => Def::Script,
            Self::Bash => Def::Shell,
            Self::Json => Def::Json,
            Self::Markdown => Def::Markdown,
            Self::Text => Def::Plain,
        }
    }
}

/// One run of text and its class; `None` is unstyled text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> {
    pub class: Option<TokenClass>,
    pub value: &'a str,
}

pub type CodeLine<'a> = Vec<Token<'a>>;

/// A classified span of the input, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Range {
    pub start: usize,
    pub end: usize,
    pub class: TokenClass,
}

impl Range {
    pub const fn new(start: usize, end: usize, class: TokenClass) -> Self {
        Self { start, end, class }
    }
}

/// A registered language definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Def {
    Script,
    Shell,
    Json,
    Markdown,
    Plain,
}

/// A language name or alias, as a fence info string names it.
fn def_named(name: &str) -> Option<Def> {
    let n = text::trim(name).to_lowercase();
    Some(match n.as_str() {
        "ts" | "typescript" | "angular-ts" => Def::Script,
        "shell" | "bash" | "sh" | "zsh" | "cmd" | "console" => Def::Shell,
        "json" | "jsonc" | "json5" => Def::Json,
        "markdown" | "md" => Def::Markdown,
        "plaintext" | "text" | "txt" | "-->" => Def::Plain,
        _ => return None,
    })
}

/// Ranges for `code` in `def`, sorted and with overlaps dropped.
fn ranges_of(code: &str, def: Def, depth: usize) -> Vec<Range> {
    if depth > 24 {
        return Vec::new();
    }
    let raw = match def {
        Def::Script => script::collect(code),
        Def::Shell => shell::collect(code),
        Def::Json => markup::json(code),
        Def::Markdown => markup::markdown(code, depth),
        Def::Plain => Vec::new(),
    };
    normalize(code.len(), raw)
}

fn normalize(len: usize, input: Vec<Range>) -> Vec<Range> {
    let mut ranges: Vec<Range> = input
        .into_iter()
        .map(|r| Range::new(r.start.min(len), r.end.min(len), r.class))
        .filter(|r| r.start < r.end)
        .collect();
    if ranges.windows(2).any(|w| w[1].start < w[0].start) {
        ranges.sort_by_key(|r| r.start);
    }
    let mut end = 0;
    ranges.retain(|r| {
        if r.start < end {
            return false;
        }
        end = r.end;
        true
    });
    ranges
}

fn to_tokens<'a>(code: &'a str, ranges: &[Range]) -> Vec<Token<'a>> {
    if ranges.is_empty() {
        return vec![Token {
            class: None,
            value: code,
        }];
    }
    let mut out = Vec::with_capacity(ranges.len() * 2 + 1);
    let mut at = 0;
    for r in ranges {
        if r.start > at {
            out.push(Token {
                class: None,
                value: &code[at..r.start],
            });
        }
        out.push(Token {
            class: Some(r.class),
            value: &code[r.start..r.end],
        });
        at = r.end;
    }
    if at < code.len() {
        out.push(Token {
            class: None,
            value: &code[at..],
        });
    }
    out
}

/// Whether `code` is small enough to tokenize.
pub fn is_highlightable(code: &str, lang: Lang) -> bool {
    units::len(code) <= lang.limit()
}

/// One classless token per line, for input too large to tokenize.
pub fn plain_lines(code: &str) -> Vec<CodeLine<'_>> {
    code.split('\n')
        .map(|line| {
            if line.is_empty() {
                Vec::new()
            } else {
                vec![Token {
                    class: None,
                    value: line,
                }]
            }
        })
        .collect()
}

/// Tokenize `code` and regroup the tokens into lines, since one token may
/// span several newlines.
pub fn tokenize_lines(code: &str, lang: Lang) -> Vec<CodeLine<'_>> {
    let ranges = ranges_of(code, lang.def(), 0);
    let mut lines: Vec<CodeLine<'_>> = vec![Vec::new()];
    for token in to_tokens(code, &ranges) {
        for (i, part) in token.value.split('\n').enumerate() {
            if i > 0 {
                lines.push(Vec::new());
            }
            if !part.is_empty()
                && let Some(line) = lines.last_mut()
            {
                line.push(Token {
                    class: token.class,
                    value: part,
                });
            }
        }
    }
    lines
}
