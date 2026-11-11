//! Collapse home directory prefixes to `~`: `/home/<user>`, `/Users/<user>`,
//! `C:\Users\<user>`, WSL and `file://` forms, and Claude and Pi project
//! slugs. The known home is collapsed exactly first; the generic patterns
//! after it are best effort.
//!
//! The TS patterns use lookbehind and lookahead, which the `regex` crate
//! lacks: each pattern here matches the body, and the char before and after
//! the match are checked by hand. A failed check retries one char on, as a
//! failed lookaround does.

use std::sync::LazyLock;

use regex::{Captures, Regex};

/// `\p{L}\p{M}\p{N}_` inside a class.
const WORD: &str = r"\p{L}\p{M}\p{N}_";

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|e| unreachable!("bad pattern {pattern}: {e}"))
}

static WORD_CHAR: LazyLock<Regex> = LazyLock::new(|| re(&format!("^[{WORD}]$")));

fn is_word(c: char) -> bool {
    let mut buf = [0u8; 4];
    WORD_CHAR.is_match(c.encode_utf8(&mut buf))
}

fn before(text: &str, at: usize) -> Option<char> {
    text[..at].chars().next_back()
}

fn after(text: &str, at: usize) -> Option<char> {
    text[at..].chars().next()
}

/// `NAME(?: NAME)*` as matched, cut to the longest prefix of whole names
/// followed by `\` or `/`, else the first name: the TS `SPACED_NAME`.
fn spaced_end(text: &str, start: usize, end: usize) -> usize {
    let chain = &text[start..end];
    let first = chain.find(' ').map_or(end, |i| start + i);
    let mut ends = chain
        .match_indices(' ')
        .map(|(i, _)| start + i)
        .chain(std::iter::once(end));
    ends.rfind(|&e| matches!(after(text, e), Some('\\' | '/')))
        .unwrap_or(first)
}

/// One home pattern with its hand-checked context.
struct Pattern {
    re: Regex,
    /// The match start is acceptable given the text before it.
    behind: fn(&str, usize) -> bool,
    /// Where the match really ends, or `None` when the text after rejects it.
    end: fn(&str, &Captures<'_>) -> Option<usize>,
    to: &'static str,
}

fn whole(_: &str, c: &Captures<'_>) -> Option<usize> {
    c.get(0).map(|m| m.end())
}

fn any_before(_: &str, _: usize) -> bool {
    true
}

impl Pattern {
    fn replace(&self, text: &str) -> String {
        let mut out = String::new();
        let mut last = 0;
        let mut at = 0;
        while at <= text.len() {
            let Some(c) = self.re.captures_at(text, at) else {
                break;
            };
            let Some(m) = c.get(0) else { break };
            let end = if (self.behind)(text, m.start()) {
                (self.end)(text, &c)
            } else {
                None
            };
            match end {
                Some(end) if end > m.start() => {
                    out.push_str(&text[last..m.start()]);
                    out.push_str(self.to);
                    last = end;
                    at = end;
                }
                _ => at = m.start() + after(text, m.start()).map_or(1, char::len_utf8),
            }
        }
        if last == 0 {
            return text.to_owned();
        }
        out.push_str(&text[last..]);
        out
    }
}

const NAME: &str = r"[\p{L}\p{M}\p{N}_@-]+(?:\.[\p{L}\p{M}\p{N}_@-]+)*";

/// `(?<=\/|:\/\/file)`: a slash, or `://file`, right before.
fn after_slash_or_file(text: &str, at: usize) -> bool {
    text[..at].ends_with('/') || text[..at].ends_with("://file")
}

/// End of a `UNIX_PATH` match: the `Users/` branch keeps whole names only.
fn unix_path_end(text: &str, c: &Captures<'_>) -> Option<usize> {
    let m = c.get(0)?;
    Some(
        c.name("spaced")
            .map_or(m.end(), |s| spaced_end(text, s.start(), s.end())),
    )
}

fn unix_path() -> String {
    format!(r"(?:/var)?/(?:home/{NAME}|Users/(?P<spaced>{NAME}(?: {NAME})*))")
}

static GENERIC: LazyLock<Vec<Pattern>> = LazyLock::new(|| {
    vec![
        Pattern {
            re: re(&unix_path()),
            behind: after_slash_or_file,
            end: unix_path_end,
            to: "/~",
        },
        Pattern {
            re: re(&unix_path()),
            behind: |t, at| before(t, at).is_none_or(|c| !(is_word(c) || c == '.' || c == '/')),
            end: unix_path_end,
            to: "~",
        },
        Pattern {
            re: re(&format!(
                r"(?i)(?:[A-Za-z]:(?:\\{{1,2}}|/)|/(?:mnt/)?[A-Za-z]/)users(?:\\{{1,2}}|/)(?P<spaced>{NAME}(?: {NAME})*)"
            )),
            behind: |t, at| before(t, at).is_none_or(|c| !(is_word(c) || c == '.')),
            end: unix_path_end,
            to: "~",
        },
        Pattern {
            re: re(&format!(
                r"(?i)\\{{2,4}}wsl(?:\.localhost|\$)\\{{1,2}}[^\t\n\x0B\x0C\r \x{{A0}}\x{{1680}}\x{{2000}}-\x{{200A}}\x{{2028}}\x{{2029}}\x{{202F}}\x{{205F}}\x{{3000}}\x{{FEFF}}\\/]+\\{{1,2}}home\\{{1,2}}{NAME}"
            )),
            behind: any_before,
            end: whole,
            to: "~",
        },
        Pattern {
            re: re(&format!(r"--(?:home|Users|[A-Za-z]--[Uu]sers)-[{WORD}@]+")),
            behind: |t, at| before(t, at).is_none_or(|c| !is_word(c)),
            end: |t, c| {
                let m = c.get(0)?;
                // `(?=[\w@.-]*--(?![\w@.-]))`: the run of name chars after the
                // match ends in `--`.
                let run_end = t[m.end()..]
                    .char_indices()
                    .find(|&(_, ch)| !(is_word(ch) || matches!(ch, '@' | '.' | '-')))
                    .map_or(t.len(), |(i, _)| m.end() + i);
                t[m.end()..run_end].ends_with("--").then_some(m.end())
            },
            to: "--~",
        },
        Pattern {
            re: re(&format!(
                r"(?:-(?:home|Users)|[A-Za-z]--[Uu]sers)-[{WORD}@]+"
            )),
            behind: |t, at| before(t, at).is_none_or(|c| !(is_word(c) || c == '-')),
            end: whole,
            to: "-~",
        },
    ]
});

/// `[\\/]+` split, empty parts dropped.
fn segments(home: &str) -> Vec<&str> {
    home.split(['\\', '/']).filter(|s| !s.is_empty()).collect()
}

/// How Claude names a project dir: every UTF-16 unit outside `[a-zA-Z0-9]`
/// becomes `-`.
fn slug_of(path: &str) -> String {
    path.chars()
        .flat_map(|c| {
            let n = if c.is_ascii_alphanumeric() {
                0
            } else {
                c.len_utf16()
            };
            std::iter::repeat_n('-', n).chain(c.is_ascii_alphanumeric().then_some(c))
        })
        .collect()
}

/// How Pi names one: `--<path>--`, only separators and `:` become `-`.
fn pi_slug_of(path: &str) -> String {
    let rest = path
        .strip_prefix(['\\', '/'])
        .unwrap_or(path)
        .replace(['\\', '/', ':'], "-");
    format!("--{rest}")
}

fn not_word_or_dash_after(t: &str, c: &Captures<'_>) -> Option<usize> {
    let m = c.get(0)?;
    after(t, m.end())
        .is_none_or(|ch| !(is_word(ch) || ch == '-'))
        .then_some(m.end())
}

/// Exact matchers for a known home, or `None` when it has fewer than two
/// segments.
fn exact_patterns(home: &str) -> Option<Vec<Pattern>> {
    let segs = segments(home);
    if segs.len() < 2 {
        return None;
    }
    let trimmed = home.trim_end_matches(['\\', '/']);
    let rooted = home.starts_with('/');
    let sep = if home.contains('\\') {
        r"(?:\\{1,2}|/)"
    } else {
        "/"
    };
    let body = format!(
        "{}{}",
        if rooted { "/" } else { "" },
        segs.iter()
            .map(|s| regex::escape(s))
            .collect::<Vec<_>>()
            .join(sep)
    );
    let b = home.as_bytes();
    let drive = b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
    let flags = if drive { "(?i)" } else { "" };
    let mut out = Vec::new();
    if rooted {
        out.push(Pattern {
            re: re(&format!("{flags}{body}")),
            behind: after_slash_or_file,
            end: not_word_or_dash_after,
            to: "/~",
        });
    }
    out.push(Pattern {
        re: re(&format!("{flags}{body}")),
        behind: if rooted {
            |t, at| before(t, at).is_none_or(|c| !(is_word(c) || c == '.' || c == '/'))
        } else {
            |t, at| before(t, at).is_none_or(|c| !(is_word(c) || c == '.'))
        },
        end: not_word_or_dash_after,
        to: "~",
    });
    out.push(Pattern {
        re: re(&format!("{flags}{}", regex::escape(&pi_slug_of(trimmed)))),
        behind: |t, at| before(t, at).is_none_or(|c| !is_word(c)),
        end: |t, c| {
            let m = c.get(0)?;
            after(t, m.end())
                .is_none_or(|ch| !is_word(ch))
                .then_some(m.end())
        },
        to: "--~",
    });
    out.push(Pattern {
        re: re(&format!("{flags}{}", regex::escape(&slug_of(trimmed)))),
        behind: |t, at| before(t, at).is_none_or(|c| !(is_word(c) || c == '-')),
        end: |t, c| {
            let m = c.get(0)?;
            after(t, m.end())
                .is_none_or(|ch| !ch.is_ascii_alphanumeric())
                .then_some(m.end())
        },
        to: "-~",
    });
    Some(out)
}

/// Replaces home directory prefixes with `~`, anywhere in a text.
pub struct HomeCollapser {
    exact: Vec<Pattern>,
}

impl HomeCollapser {
    /// `home` is collapsed exactly first, in path and slug form.
    pub fn new(home: Option<&str>) -> Self {
        Self {
            exact: home.and_then(exact_patterns).unwrap_or_default(),
        }
    }

    pub fn collapse(&self, text: &str) -> String {
        let known = self
            .exact
            .iter()
            .fold(text.to_owned(), |t, p| p.replace(&t));
        GENERIC.iter().fold(known, |t, p| p.replace(&t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(home: Option<&str>, text: &str) -> String {
        HomeCollapser::new(home).collapse(text)
    }

    #[test]
    fn generic_forms() {
        assert_eq!(c(None, "/Users/demo/proj and /home/x/y"), "~/proj and ~/y");
        assert_eq!(c(None, "file:///home/x/y"), "file:///~/y");
        assert_eq!(c(None, r"C:\Users\John Smith\x"), r"~\x");
        assert_eq!(c(None, "-Users-demo-proj"), "-~-proj");
        assert_eq!(c(None, "--home-u-work--"), "--~-work--");
        assert_eq!(c(None, "--home-dir"), "--home-dir");
        assert_eq!(c(None, "a/Users/x"), "a/Users/x");
    }

    #[test]
    fn exact_home() {
        assert_eq!(
            c(Some("/home/jo-ann"), "/home/jo-ann/x -home-jo-ann-x"),
            "~/x -~-x"
        );
        assert_eq!(c(Some("/home/jo-ann"), "/home/jo-annx"), "~");
    }
}
