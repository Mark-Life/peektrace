//! The shell's view of each char of a command: code, quoted, substituted or
//! heredoc body. Indexes are byte offsets; every char the lexer reacts to is
//! ASCII.

use crate::js::regex::LazyRe;
use crate::js_re;

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
        if code
            && c == b'<'
            && let Some(past) = self.heredoc_op(i)
        {
            return past;
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
        let escape = c == b'\\' && ctx[i].is_code() && ctx.get(i + 1) == Some(&Ctx::Quoted);
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
