//! Shell lexing: segments, words, wrapper unwrapping and the per-char
//! context (code, quoted, heredoc) the detectors ask about. Indexes are byte
//! offsets; every char the lexer reacts to is ASCII.

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

/// What the shell makes of one char.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctx {
    Code,
    Subst,
    Quoted,
    Heredoc,
}

impl Ctx {
    pub const fn is_code(self) -> bool {
        matches!(self, Self::Code | Self::Subst)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Frame {
    Single,
    Double,
    Subst,
    Paren,
}

static HEREDOC_OP: LazyRe = js_re!(r#"^<<(-?)[ \t]*(?:'([^'\n]*)'|"([^"\n]*)"|\\?([\w.-]+))"#);

struct Pending {
    delim: String,
    strip_tabs: bool,
}

fn heredoc_end(cmd: &str, from: usize, doc: &Pending) -> usize {
    let mut at = from;
    while at < cmd.len() {
        let nl = cmd[at..].find('\n').map(|n| n + at);
        let end = nl.unwrap_or(cmd.len());
        let line = &cmd[at..end];
        let next = nl.map_or(cmd.len(), |n| n + 1);
        let line = if doc.strip_tabs {
            line.trim_start_matches('\t')
        } else {
            line
        };
        if line == doc.delim {
            return next;
        }
        at = next;
    }
    cmd.len()
}

struct Lexer<'a> {
    cmd: &'a str,
    escaped: bool,
    ops: Vec<(usize, usize)>,
    out: Vec<Ctx>,
    pending: Vec<Pending>,
    stack: Vec<Frame>,
}

impl Lexer<'_> {
    fn in_code(&self) -> bool {
        matches!(self.stack.last(), None | Some(Frame::Subst | Frame::Paren))
    }

    fn code_ctx(&self) -> Ctx {
        if self.stack.contains(&Frame::Double) {
            Ctx::Subst
        } else {
            Ctx::Code
        }
    }

    fn step_frames(&mut self, c: u8, prev: u8) {
        match self.stack.last() {
            Some(Frame::Single) => {
                if c == b'\'' {
                    self.stack.pop();
                }
            }
            Some(Frame::Double) => {
                if c == b'"' {
                    self.stack.pop();
                } else if c == b'(' && prev == b'$' {
                    self.stack.push(Frame::Subst);
                }
            }
            top => match c {
                b'\'' => self.stack.push(Frame::Single),
                b'"' => self.stack.push(Frame::Double),
                b'(' => self.stack.push(if prev == b'$' {
                    Frame::Subst
                } else {
                    Frame::Paren
                }),
                b')' if top.is_some() => {
                    self.stack.pop();
                }
                _ => {}
            },
        }
    }

    fn heredoc_op(&mut self, i: usize) -> Option<usize> {
        let b = self.cmd.as_bytes();
        if b.get(i + 1) != Some(&b'<') || b.get(i + 2) == Some(&b'<') {
            return None;
        }
        let m = HEREDOC_OP.captures(&self.cmd[i..])?;
        let len = m.end();
        self.pending.push(Pending {
            delim: m.get(2).or(m.get(3)).or(m.get(4)).unwrap_or("").to_owned(),
            strip_tabs: m.get(1) == Some("-"),
        });
        let ctx = self.out[i];
        self.out[i + 1..i + len].fill(ctx);
        self.ops.push((i, i + len));
        Some(i + len)
    }

    fn heredoc_bodies(&mut self, i: usize) -> usize {
        let mut at = i + 1;
        for doc in std::mem::take(&mut self.pending) {
            let end = heredoc_end(self.cmd, at, &doc);
            self.out[at..end].fill(Ctx::Heredoc);
            at = end;
        }
        at
    }

    fn lex_char(&mut self, i: usize) -> usize {
        let b = self.cmd.as_bytes();
        let c = b[i];
        if self.escaped {
            self.out[i] = Ctx::Quoted;
            self.escaped = false;
            return i + 1;
        }
        let code = self.in_code();
        self.out[i] = if code { self.code_ctx() } else { Ctx::Quoted };
        if c == b'\\' && self.stack.last() != Some(&Frame::Single) {
            self.escaped = true;
            return i + 1;
        }
        if code && c == b'<' {
            if let Some(past) = self.heredoc_op(i) {
                return past;
            }
        }
        if code && c == b'\n' && !self.pending.is_empty() {
            return self.heredoc_bodies(i);
        }
        let prev = i.checked_sub(1).map_or(0, |p| b[p]);
        self.step_frames(c, prev);
        i + 1
    }
}

fn lex(cmd: &str) -> Lexer<'_> {
    let mut lx = Lexer {
        cmd,
        escaped: false,
        ops: Vec::new(),
        out: vec![Ctx::Code; cmd.len()],
        pending: Vec::new(),
        stack: Vec::new(),
    };
    let mut i = 0;
    while i < cmd.len() {
        i = lx.lex_char(i);
    }
    lx
}

/// The context of every byte of `cmd`.
pub fn shell_contexts(cmd: &str) -> Vec<Ctx> {
    lex(cmd).out
}

fn utf8(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// Each heredoc operator becomes `<<HEREDOC`; bodies and delimiter lines go.
pub fn strip_heredocs(cmd: &str) -> String {
    if !cmd.contains("<<") {
        return cmd.to_owned();
    }
    let lx = lex(cmd);
    let b = cmd.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut op = 0;
    let mut i = 0;
    while i < b.len() {
        let (start, end) = lx.ops.get(op).copied().unwrap_or((usize::MAX, 0));
        if i == start {
            out.extend_from_slice(b"<<HEREDOC");
            i = end;
            op += 1;
            continue;
        }
        if lx.out[i] != Ctx::Heredoc {
            out.push(b[i]);
        }
        i += 1;
    }
    utf8(out)
}

/// True when the shell reads byte `at` of `cmd` as code.
pub fn unquoted_at(cmd: &str, at: usize) -> bool {
    let end = (at + 1).min(cmd.len());
    let mut end = end;
    while !cmd.is_char_boundary(end) {
        end += 1;
    }
    lex(&cmd[..end]).out.get(at).is_some_and(|c| c.is_code())
}

/// `cmd` without its shell backslash escapes, and the source index of each byte.
pub struct Unescaped {
    pub text: String,
    pub from: Vec<usize>,
}

pub fn unescaped(cmd: &str) -> Unescaped {
    let ctx = shell_contexts(cmd);
    let b = cmd.as_bytes();
    let mut text = Vec::with_capacity(b.len());
    let mut from = Vec::with_capacity(b.len());
    for (i, &c) in b.iter().enumerate() {
        let escape = c == b'\\'
            && ctx[i].is_code()
            && ctx.get(i + 1) == Some(&Ctx::Quoted);
        if !escape {
            text.push(c);
            from.push(i);
        }
    }
    Unescaped {
        text: utf8(text),
        from,
    }
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
