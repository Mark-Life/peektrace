//! Shell lexing: segments, words, wrapper unwrapping and the per-char
//! context (code, quoted, heredoc) the detectors ask about. Indexes are byte
//! offsets; every char the lexer reacts to is ASCII.

pub use super::shell_ctx::{
    Ctx, Unescaped, shell_contexts, strip_heredocs, unescaped, unquoted_at,
};
use crate::js::regex::LazyRe;
use crate::js::text;
use crate::js_re;

/// One shell word, with whether it arrived quoted or substituted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub quoted: bool,
    pub text: String,
}

#[derive(Default)]
struct ScanState {
    backtick: bool,
    depth: usize,
    double: bool,
    single: bool,
}

impl ScanState {
    /// Update the flags for one char; true when it sits in a quoted or nested span.
    fn quoted(&mut self, c: char) -> bool {
        if c == '\'' && !(self.double || self.backtick) {
            self.single = !self.single;
            return true;
        }
        if c == '"' && !self.single {
            self.double = !self.double;
            return true;
        }
        if self.single || self.double {
            return true;
        }
        match c {
            '`' => {
                self.backtick = !self.backtick;
                true
            }
            '(' => {
                self.depth += 1;
                true
            }
            ')' => {
                self.depth = self.depth.saturating_sub(1);
                true
            }
            _ => self.depth > 0 || self.backtick,
        }
    }
}

/// Top-level segments of a command, and whether a lone `&` backgrounds one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Segments {
    pub segments: Vec<String>,
    pub background: bool,
}

/// Split on `&&`, `||`, `;`, `|` or a newline outside quotes and nesting.
pub fn split_segments(cmd: &str) -> Segments {
    let mut st = ScanState::default();
    let mut out = Segments::default();
    let mut buf = String::new();
    let push = |buf: &mut String, out: &mut Segments| {
        let t = text::trim(buf);
        if !t.is_empty() {
            out.segments.push(t.to_owned());
        }
        buf.clear();
    };
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let prev = i.checked_sub(1).map(|p| chars[p]);
        if c == '\\' && !st.single {
            buf.push(c);
            if let Some(n) = next {
                buf.push(n);
            }
            i += 2;
            continue;
        }
        if st.quoted(c) {
            buf.push(c);
        } else if (c == '&' && next == Some('&')) || (c == '|' && next == Some('|')) {
            push(&mut buf, &mut out);
            i += 1;
        } else if matches!(c, '|' | ';' | '\n') {
            push(&mut buf, &mut out);
        } else if c == '&' && prev != Some('>') && next != Some('>') {
            out.background = true;
            push(&mut buf, &mut out);
        } else {
            buf.push(c);
        }
        i += 1;
    }
    push(&mut buf, &mut out);
    out
}

/// Split one segment into quote-aware words, keeping `$(...)` as one word.
pub fn tokenize(segment: &str) -> Vec<Word> {
    let mut st = ScanState::default();
    let mut words = Vec::new();
    let mut buf = String::new();
    let mut quoted = false;
    let push = |buf: &mut String, quoted: &mut bool, words: &mut Vec<Word>| {
        if !buf.is_empty() {
            words.push(Word {
                text: std::mem::take(buf),
                quoted: *quoted,
            });
        }
        *quoted = false;
    };
    let mut chars = segment.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                buf.push(n);
            }
            continue;
        }
        if c == '\'' && !st.double {
            st.single = !st.single;
            quoted = true;
            continue;
        }
        if c == '"' && !st.single {
            st.double = !st.double;
            quoted = true;
            continue;
        }
        if !(st.single || st.double) {
            if c == '(' || (c == '$' && chars.peek() == Some(&'(')) {
                st.depth += 1;
            } else if c == ')' {
                st.depth = st.depth.saturating_sub(1);
            } else if st.depth == 0 && text::is_js_whitespace(c) {
                push(&mut buf, &mut quoted, &mut words);
                continue;
            }
        }
        buf.push(c);
    }
    push(&mut buf, &mut quoted, &mut words);
    words
}

static FLAG: LazyRe = js_re!(r"^-{1,2}[^-\s]");
/// Leading redirection operator, e.g. `>`, `>>`, `2>`, `&>`, `<`, `2>&1`.
pub static REDIRECT: LazyRe = js_re!(r"^(?:\d?(?:>>|>|<<<|<<|<)|&>>?)");
static ENV_ASSIGN: LazyRe = js_re!(r"^[A-Za-z_][A-Za-z0-9_]*=");
static EXTENSION: LazyRe = js_re!(r"\.[A-Za-z0-9]{1,6}$");
static NUMBER_WORD: LazyRe = js_re!(r"^\d+(?:[.,:]\d+)*$");
static HEX_WORD: LazyRe = js_re!(r"^[0-9a-f]{7,}$", "i");
static UUID_WORD: LazyRe = js_re!(
    r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
    "i"
);
static URL_WORD: LazyRe = js_re!(r"^[a-z][a-z0-9+.-]*:\/\/", "i");
static OPERAND_WORD: LazyRe = js_re!(r"^(?:\d+(?:\.\d+)?[smhd]?|\{\})$");

/// A flag such as `-v`, `--filter`, `--filter=web`, or `--`.
pub fn is_flag(w: &str) -> bool {
    FLAG.is_match(w) || w == "--"
}

/// `FOO=bar`, an env assignment in front of the command.
pub fn is_env_assign(w: &str) -> bool {
    ENV_ASSIGN.is_match(w)
}

/// The word names a file or directory rather than a keyword.
pub fn is_path_like(w: &str) -> bool {
    w.contains('/')
        || w.contains('\\')
        || w.starts_with('~')
        || w.starts_with('.')
        || EXTENSION.is_match(w)
}

/// A number, hash, UUID, variable or URL.
pub fn is_literal(w: &str) -> bool {
    NUMBER_WORD.is_match(w)
        || HEX_WORD.is_match(w)
        || UUID_WORD.is_match(w)
        || URL_WORD.is_match(w)
        || w.starts_with('$')
        || w.starts_with("<<")
}

fn last_part(w: &str) -> &str {
    w.rsplit(['/', '\\']).next().unwrap_or(w)
}

/// Basename with the extension dropped: the script's own name.
pub fn script_base(text: &str) -> String {
    EXTENSION.replace_first(last_part(text), |_| String::new())
}

/// Strip a directory prefix and a `.exe` suffix (any case) from a binary name.
pub fn bin_name(w: &str) -> &str {
    let base = last_part(w);
    let n = base.len();
    if n >= 4 && base.is_char_boundary(n - 4) && base[n - 4..].eq_ignore_ascii_case(".exe") {
        &base[..n - 4]
    } else {
        base
    }
}

/// Wrappers that run their remaining words as the command.
pub const UNARY_WRAPPERS: [&str; 13] = [
    "bunx", "command", "env", "exec", "nice", "nohup", "npx", "pnpx", "stdbuf", "sudo", "time",
    "timeout", "xargs",
];

/// A word a wrapper takes before the command, as regex source.
pub const WRAPPER_OPERAND: &str = r"(?:\d+(?:\.\d+)?[smhd]?|\{\})";

fn binary_wrapper(head: &str, next: &str) -> bool {
    match head {
        "bun" => next == "x",
        "npm" => matches!(next, "exec" | "x"),
        "pnpm" => matches!(next, "dlx" | "exec"),
        "yarn" => next == "dlx",
        _ => false,
    }
}

const UNWRAP_GUARD: usize = 4;

/// Index of the real command among a segment's words, past env assignments
/// and wrappers; `words.len()` when there is none.
pub fn command_index<S: AsRef<str>>(words: &[S]) -> usize {
    let at = |i: usize| words.get(i).map_or("", AsRef::as_ref);
    let mut i = 0;
    while i < words.len() && is_env_assign(at(i)) {
        i += 1;
    }
    for _ in 0..UNWRAP_GUARD {
        if i >= words.len() {
            break;
        }
        let head = bin_name(at(i));
        if UNARY_WRAPPERS.contains(&head) {
            i += 1;
            while i < words.len()
                && (is_env_assign(at(i)) || is_flag(at(i)) || OPERAND_WORD.is_match(at(i)))
            {
                i += 1;
            }
            continue;
        }
        if binary_wrapper(head, bin_name(at(i + 1))) {
            i += 2;
            continue;
        }
        break;
    }
    i
}

/// The binaries a command runs, one per segment, in first-run order.
pub fn ran_bins(cmd: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for segment in split_segments(cmd).segments {
        let words: Vec<String> = tokenize(&segment).into_iter().map(|w| w.text).collect();
        if let Some(w) = words.get(command_index(&words)) {
            let bin = bin_name(w);
            if !out.iter().any(|b| b == bin) {
                out.push(bin.to_owned());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_respect_quotes() {
        let s = split_segments(r#"grep -E "a|b" x && echo hi; sleep 1 &"#);
        assert_eq!(s.segments, vec![r#"grep -E "a|b" x"#, "echo hi", "sleep 1"]);
        assert!(s.background);
    }

    #[test]
    fn heredoc_stripped() {
        assert_eq!(
            strip_heredocs("cat <<'EOF' > f\nbody\nEOF\necho x"),
            "cat <<HEREDOC > f\necho x"
        );
    }

    #[test]
    fn command_index_unwraps() {
        assert_eq!(command_index(&["FOO=1", "timeout", "30s", "npx", "tsc"]), 4);
        assert_eq!(command_index(&["bun", "x", "tsc"]), 2);
    }

    #[test]
    fn ran_bins_dedups() {
        assert_eq!(ran_bins("/usr/bin/jq . a | jq . b; rg x"), vec!["jq", "rg"]);
    }
}
