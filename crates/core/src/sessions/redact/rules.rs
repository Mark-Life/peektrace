//! The redaction rules, in the order they run. The patterns follow the TS regexes with JS semantics spelled out: `\s` is
//! the JS whitespace set, `\b` and case folding are ASCII-only (no `u`
//! flag), and offsets for the keyword lookback count UTF-16 units.

use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::js::{text, units};

const ENTROPY_AWS: f64 = 4.0;
const ENTROPY_B64: f64 = 3.5;
const ENTROPY_HEX: f64 = 3.0;
const CRED_LOOKBACK: usize = 48;
const MAX_FALLBACK_LEN: usize = 512;

/// The chars JS `\s` matches, for use inside a class.
const WS: &str = r"\t\n\x0B\x0C\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}";

/// ASCII word boundary, as JS `\b` without the `u` flag.
const B: &str = r"(?-u:\b)";

/// `pattern` with every ASCII letter outside a class matching both cases,
/// as the JS `i` flag does without `u`. Classes are written with both cases
/// already.
pub(super) fn ascii_ci(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len() * 2);
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push(c);
                let Some(next) = chars.next() else { break };
                out.push(next);
                if next == 'x' && chars.peek() == Some(&'{') {
                    for d in chars.by_ref() {
                        out.push(d);
                        if d == '}' {
                            break;
                        }
                    }
                }
            }
            '[' => {
                out.push(c);
                while let Some(d) = chars.next() {
                    out.push(d);
                    if d == '\\' {
                        if let Some(e) = chars.next() {
                            out.push(e);
                        }
                    } else if d == ']' {
                        break;
                    }
                }
            }
            '(' if chars.peek() == Some(&'?') => {
                out.push(c);
                for d in chars.by_ref() {
                    out.push(d);
                    if d == ':' || d == ')' {
                        break;
                    }
                }
            }
            c if c.is_ascii_alphabetic() => {
                out.push('[');
                out.push(c.to_ascii_lowercase());
                out.push(c.to_ascii_uppercase());
                out.push(']');
            }
            c => out.push(c),
        }
    }
    out
}

fn re(pattern: &str) -> Regex {
    // The patterns are constants; a bad one is a bug caught by every test.
    Regex::new(pattern).unwrap_or_else(|e| unreachable!("bad pattern {pattern}: {e}"))
}

/// Shannon entropy (bits per char) over code points, divided by UTF-16
/// length as JS does.
fn entropy(s: &str) -> f64 {
    let mut freq: Vec<(char, usize)> = Vec::new();
    for c in s.chars() {
        match freq.iter_mut().find(|(k, _)| *k == c) {
            Some(slot) => slot.1 += 1,
            None => freq.push((c, 1)),
        }
    }
    #[expect(clippy::cast_precision_loss, reason = "JS numbers are f64")]
    let len = units::len(s) as f64;
    freq.iter().fold(0.0, |h, &(_, n)| {
        #[expect(clippy::cast_precision_loss, reason = "JS numbers are f64")]
        let p = n as f64 / len;
        h - p * p.log2()
    })
}

static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| {
    let body = ascii_ci(
        r"^(?:null|none|true|false|undefined|changeme|change-me|example|examples?|sample|placeholder|redacted|dummy|fixme|todo|foo|bar|baz|value|your[-_a-zA-Z0-9]*|my[-_a-zA-Z0-9]*|test|xxx+|\*+|<[^>]+>|\$\{?[A-Za-z0-9_]+\}?|process\.env(?:\.[A-Za-z_]+)?|os\.environ[^\n\r\x{2028}\x{2029}]*)$",
    );
    re(&body)
});

static CRED_CONTEXT: LazyLock<Regex> = LazyLock::new(|| {
    re(&ascii_ci(&format!(
        r"(?:secret|token|api[_-]?key|apikey|access[_-]?key|private[_-]?key|password|passwd|{B}pwd{B}|credential|client[_-]?secret|signing|webhook|{B}auth|bearer)"
    )))
});

/// Env-var refs, booleans and obvious dummy values are never redacted.
fn is_placeholder(v: &str) -> bool {
    PLACEHOLDER.is_match(v)
        || (!v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        || v.starts_with("[REDACTED")
}

/// True when a credential keyword sits in the 48 UTF-16 units before byte
/// offset `at` of `src`.
fn cred_context_at(src: &str, at: usize) -> bool {
    // 48 units span at most 48 chars; 400 bytes covers them even when every
    // char is an escaped 4-byte char.
    let mut from = at.saturating_sub(400);
    while !src.is_char_boundary(from) {
        from += 1;
    }
    let window = &src[from..at];
    let n = units::len(window);
    let tail = units::slice(window, n.saturating_sub(CRED_LOOKBACK), n);
    CRED_CONTEXT.is_match(&text::to_display(&tail))
}

/// What a context-sensitive replacer does with one match.
enum Outcome {
    /// The match stands as it is.
    Keep,
    Replace(String),
    /// No match starts here (a failed lookahead); search again one char on.
    Reject,
}

/// Replace every match of `re` in `src`, left to right, without overlaps.
fn replace_each(re: &Regex, src: &str, mut f: impl FnMut(&Captures<'_>) -> Outcome) -> String {
    let mut out = String::new();
    let mut last = 0;
    let mut at = 0;
    while at <= src.len() {
        let Some(c) = re.captures_at(src, at) else {
            break;
        };
        let Some(m) = c.get(0) else { break };
        let step = |i: usize| i + src[i..].chars().next().map_or(1, char::len_utf8);
        match f(&c) {
            Outcome::Keep => {
                at = if m.end() > m.start() {
                    m.end()
                } else {
                    step(m.start())
                }
            }
            Outcome::Replace(r) => {
                out.push_str(&src[last..m.start()]);
                out.push_str(&r);
                last = m.end();
                at = if m.end() > m.start() {
                    m.end()
                } else {
                    step(m.start())
                };
            }
            Outcome::Reject => at = step(m.start()),
        }
    }
    if last == 0 {
        return src.to_owned();
    }
    out.push_str(&src[last..]);
    out
}

fn group<'a>(c: &Captures<'a>, i: usize) -> &'a str {
    c.get(i).map_or("", |m| m.as_str())
}

/// How one rule rewrites text.
pub(super) enum Action {
    Static(&'static str),
    Dynamic(fn(&Regex, &str) -> String),
}

pub(super) struct Rule {
    pub(super) re: Regex,
    pub(super) action: Action,
}

fn openai_key(re: &Regex, src: &str) -> String {
    replace_each(re, src, |c| {
        let m = group(c, 0);
        if m["sk-".len()..].starts_with("ant-") {
            Outcome::Reject
        } else {
            Outcome::Replace("[REDACTED:openai-key]".to_owned())
        }
    })
}

fn bearer(re: &Regex, src: &str) -> String {
    replace_each(re, src, |c| {
        Outcome::Replace(format!("{}[REDACTED:bearer-token]", group(c, 1)))
    })
}

fn basic_auth(re: &Regex, src: &str) -> String {
    replace_each(re, src, |c| {
        Outcome::Replace(format!("{}[REDACTED:basic-credentials]", group(c, 1)))
    })
}

fn url_credentials(re: &Regex, src: &str) -> String {
    replace_each(re, src, |c| {
        Outcome::Replace(format!("{}://{}:[REDACTED]@", group(c, 1), group(c, 2)))
    })
}

fn aws_secret(re: &Regex, src: &str) -> String {
    replace_each(re, src, |c| {
        if entropy(group(c, 2)) >= ENTROPY_AWS {
            Outcome::Replace(format!(
                "{}[REDACTED:aws-secret-key]{}",
                group(c, 1),
                group(c, 3)
            ))
        } else {
            Outcome::Keep
        }
    })
}

fn twilio(re: &Regex, src: &str) -> String {
    let mut named = None;
    replace_each(re, src, |_| {
        if *named.get_or_insert_with(|| src.to_ascii_lowercase().contains("twilio")) {
            Outcome::Replace("[REDACTED:twilio-id]".to_owned())
        } else {
            Outcome::Keep
        }
    })
}

fn credential(re: &Regex, src: &str) -> String {
    replace_each(re, src, |c| {
        if is_placeholder(group(c, 3)) {
            Outcome::Keep
        } else {
            Outcome::Replace(format!(
                "{}{}[REDACTED:credential]",
                group(c, 1),
                group(c, 2)
            ))
        }
    })
}

const ENV_SENSITIVE: [&str; 18] = [
    "KEY",
    "KEYS",
    "TOKEN",
    "TOKENS",
    "SECRET",
    "SECRETS",
    "PASSWORD",
    "PASSWD",
    "PWD",
    "PASS",
    "CRED",
    "CREDS",
    "CREDENTIAL",
    "CREDENTIALS",
    "APIKEY",
    "PRIVATE",
    "PASSPHRASE",
    "SIGNINGKEY",
];
const ENV_DESCRIPTIVE: [&str; 24] = [
    "VERSION", "URL", "URI", "HOST", "PORT", "PATH", "DIR", "NAME", "ENABLED", "DISABLED", "MODE",
    "ENV", "REGION", "BASE", "ENDPOINT", "PROVIDER", "TYPE", "PUBLIC", "TIMEOUT", "COUNT", "LEVEL",
    "FORMAT", "PREFIX", "SUFFIX",
];

/// `v.replace(/^["']|["']$/g, "")`.
fn quote_trim(v: &str) -> &str {
    let quote = |c: char| c == '"' || c == '\'';
    let v = v.strip_prefix(quote).unwrap_or(v);
    v.strip_suffix(quote).unwrap_or(v)
}

/// JS line terminators, where multi-line `^` and `$` match.
fn is_line_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `KEY=value` lines with a sensitive key: the multi-line JS rule, applied
/// to each line on its own since `.` stops at every JS line break.
fn env_secret(re: &Regex, src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    loop {
        let end = rest.find(is_line_break).unwrap_or(rest.len());
        let line = &rest[..end];
        match re.captures(line) {
            Some(c) => {
                let (key, sep, val) = (group(&c, 1), group(&c, 2), group(&c, 3));
                let sensitive = key.split('_').any(|s| ENV_SENSITIVE.contains(&s));
                let descriptive = ENV_DESCRIPTIVE.iter().any(|d| key.ends_with(d));
                if !sensitive || descriptive || is_placeholder(quote_trim(val)) {
                    out.push_str(line);
                } else {
                    out.push_str(key);
                    out.push_str(sep);
                    out.push_str("[REDACTED:env-secret]");
                }
            }
            None => out.push_str(line),
        }
        let Some(brk) = rest[end..].chars().next() else {
            break;
        };
        out.push(brk);
        rest = &rest[end + brk.len_utf8()..];
    }
    out
}

/// Payload prefixes of base64 blobs (PNG, JPEG, PDF, zip, GIF, WebP, gzip).
const BLOB_MAGIC: [&str; 9] = [
    "iVBORw0KGgo",
    "/9j/",
    "JVBERi0",
    "UEsDB",
    "R0lGOD",
    "UklGR",
    "H4sI",
    "AAAA",
    "data:",
];

fn high_entropy(re: &Regex, src: &str) -> String {
    replace_each(re, src, |c| {
        let Some(m) = c.get(0) else {
            return Outcome::Keep;
        };
        let s = m.as_str();
        if s.len() > MAX_FALLBACK_LEN || BLOB_MAGIC.iter().any(|p| s.starts_with(p)) {
            return Outcome::Keep;
        }
        let digit = s.bytes().any(|b| b.is_ascii_digit());
        let alpha = s.bytes().any(|b| b.is_ascii_alphabetic());
        if !(digit && alpha) || is_placeholder(s) || !cred_context_at(src, m.start()) {
            return Outcome::Keep;
        }
        if entropy(s) >= ENTROPY_B64 {
            Outcome::Replace("[REDACTED:high-entropy]".to_owned())
        } else {
            Outcome::Keep
        }
    })
}

fn high_entropy_hex(re: &Regex, src: &str) -> String {
    replace_each(re, src, |c| {
        let Some(m) = c.get(0) else {
            return Outcome::Keep;
        };
        let s = m.as_str();
        let hex_alpha = s.bytes().any(|b| matches!(b, b'a'..=b'f' | b'A'..=b'F'));
        let digit = s.bytes().any(|b| b.is_ascii_digit());
        if !hex_alpha || !digit || !cred_context_at(src, m.start()) {
            return Outcome::Keep;
        }
        if entropy(s) >= ENTROPY_HEX {
            Outcome::Replace("[REDACTED:high-entropy-hex]".to_owned())
        } else {
            Outcome::Keep
        }
    })
}

pub(super) static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let key_kind = "(?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?";
    let s = |re_src: String, to: &'static str| Rule {
        re: re(&re_src),
        action: Action::Static(to),
    };
    let d = |re_src: String, f: fn(&Regex, &str) -> String| Rule {
        re: re(&re_src),
        action: Action::Dynamic(f),
    };
    vec![
        s(
            format!(
                r"-----BEGIN {key_kind}PRIVATE KEY-----(?s:.)*?-----END {key_kind}PRIVATE KEY-----"
            ),
            "[REDACTED:private-key-block]",
        ),
        s(
            r"sk-ant-[A-Za-z0-9_-]{40,}".to_owned(),
            "[REDACTED:anthropic-key]",
        ),
        d(
            format!(r"{B}sk-(?:proj-|svcacct-|admin-)?[A-Za-z0-9_-]{{20,}}"),
            openai_key,
        ),
        s(
            format!(r"{B}(?:gh[opusr]_[A-Za-z0-9]{{36}}|github_pat_[A-Za-z0-9_]{{82}}){B}"),
            "[REDACTED:github-token]",
        ),
        s(
            format!(r"{B}glpat-[A-Za-z0-9_-]{{20}}{B}"),
            "[REDACTED:gitlab-token]",
        ),
        s(
            format!(r"{B}(?:AKIA|ASIA|AGPA|AIDA|AROA|AIPA|ANPA|ABIA|ACCA)[A-Z0-9]{{16}}{B}"),
            "[REDACTED:aws-access-key-id]",
        ),
        s(
            format!(r"{B}AIza[A-Za-z0-9_-]{{35}}{B}"),
            "[REDACTED:google-api-key]",
        ),
        s(
            format!(r"{B}ya29\.[A-Za-z0-9_-]{{20,}}"),
            "[REDACTED:google-oauth-token]",
        ),
        s(
            format!(r"{B}xox[baprse]-[A-Za-z0-9-]{{10,48}}{B}"),
            "[REDACTED:slack-token]",
        ),
        s(
            format!(r"{B}xapp-[0-9]-[A-Za-z0-9-]{{10,}}{B}"),
            "[REDACTED:slack-token]",
        ),
        s(
            r"https://hooks\.slack\.com/services/T[A-Za-z0-9]+/B[A-Za-z0-9]+/[A-Za-z0-9]{24}"
                .to_owned(),
            "[REDACTED:slack-webhook]",
        ),
        s(
            format!(r"{B}(?:sk|rk|pk)_(?:live|test)_[A-Za-z0-9]{{10,99}}{B}"),
            "[REDACTED:stripe-key]",
        ),
        s(
            format!(r"{B}whsec_[A-Za-z0-9]{{20,}}{B}"),
            "[REDACTED:stripe-webhook-secret]",
        ),
        s(
            format!(r"{B}SG\.[A-Za-z0-9_-]{{22}}\.[A-Za-z0-9_-]{{43}}{B}"),
            "[REDACTED:sendgrid-key]",
        ),
        s(
            format!(r"{B}npm_[A-Za-z0-9]{{36}}{B}"),
            "[REDACTED:npm-token]",
        ),
        s(
            format!(r"{B}dop_v1_[a-f0-9]{{64}}{B}"),
            "[REDACTED:digitalocean-token]",
        ),
        s(
            format!(r"{B}hf_[A-Za-z0-9]{{30,}}{B}"),
            "[REDACTED:huggingface-token]",
        ),
        s(
            format!(r"{B}eyJ[A-Za-z0-9_-]{{10,}}\.[A-Za-z0-9_-]{{6,}}\.[A-Za-z0-9_-]*{B}"),
            "[REDACTED:jwt]",
        ),
        d(
            format!(r"{B}(Bearer[{WS}]+)[A-Za-z0-9._~+/-]{{20,}}={{0,2}}"),
            bearer,
        ),
        d(
            ascii_ci(&format!(
                r#"(Authorization"?[{WS}]*:[{WS}]*"?Basic[{WS}]+)[A-Za-z0-9+/]{{16,}}={{0,2}}"#
            )),
            basic_auth,
        ),
        d(
            ascii_ci(&format!(
                r"{B}(https?|postgres(?:ql)?|mysql|mongodb(?:\+srv)?|rediss?|amqps?|ftp)://([^{WS}/:@]+):[^{WS}/@]+@"
            )),
            url_credentials,
        ),
        d(
            ascii_ci(r#"((?:aws|secret|access)[^\n]{0,40}?["'])([A-Za-z0-9/+=]{40})(["'])"#),
            aws_secret,
        ),
        d(format!(r"{B}(?:AC|SK)[0-9a-f]{{32}}{B}"), twilio),
        d(
            ascii_ci(&format!(
                r#"(password|passwd|pwd|secret|api[_-]?key|apikey|access[_-]?token|client[_-]?secret|auth[_-]?token|token)([{WS}]*[:=][{WS}]*)["']?([^{WS}"']{{6,}})["']?"#
            )),
            credential,
        ),
        d(
            r"^[ \t]*(?:export[ \t]+)?([A-Z][A-Z0-9_]*)([ \t]*=[ \t]*)(.+)$".to_owned(),
            env_secret,
        ),
        d(r"[A-Za-z0-9_-]{32,}={0,2}".to_owned(), high_entropy),
        d(format!(r"{B}[0-9a-fA-F]{{32,}}{B}"), high_entropy_hex),
    ]
});
