//! Command families and error signatures: the keys commands and failures
//! are grouped by.

use super::shell_lex::{
    REDIRECT, Word, bin_name, command_index, is_env_assign, is_flag, is_literal, is_path_like,
    script_base, split_segments, strip_heredocs, tokenize,
};
use crate::js::regex::LazyRe;
use crate::js::{text, units};
use crate::js_re;

/// A command reduced to its binary and its flag-collapsed family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedCommand {
    pub bin: String,
    pub family: String,
}

const SIG_MAX: usize = 200;
const FAMILY_MAX: usize = 120;
const FAMILY_BUDGET: usize = 3;

const SETUP_BINS: [&str; 15] = [
    ".", "alias", "cd", "echo", "exec", "export", "popd", "pushd", "set", "shopt", "source",
    "trap", "true", "umask", "unset",
];
const SCRIPT_RUNNERS: [&str; 18] = [
    "bash", "bun", "deno", "fish", "node", "perl", "php", "poetry", "py", "python", "python3",
    "ruby", "sh", "ts-node", "tsx", "uv", "uvx", "zsh",
];
const RUN_WORDS: [&str; 8] = [
    "dlx",
    "exec",
    "run",
    "run-script",
    "task",
    "test",
    "workspace",
    "x",
];
const MESSAGE_BINS: [&str; 15] = [
    "awk", "cat", "cp", "find", "grep", "head", "ls", "mkdir", "mv", "rm", "sed", "sort", "stat",
    "tail", "wc",
];

fn meaningful_segment(segments: &[String]) -> &str {
    segments
        .iter()
        .find(|s| {
            tokenize(s)
                .into_iter()
                .find(|w| !is_env_assign(&w.text))
                .is_some_and(|w| !SETUP_BINS.contains(&bin_name(&w.text)))
        })
        .or(segments.first())
        .map_or("", String::as_str)
}

#[derive(Default)]
struct FamilyScan {
    kept: Vec<String>,
    pending_flag_value: bool,
    pending_redirect_target: bool,
    saw_operand: bool,
    saw_script: bool,
}

fn step_family(scan: &mut FamilyScan, word: &Word, bin: &str) {
    let text = word.text.as_str();
    if let Some(r) = REDIRECT.find(text) {
        scan.pending_redirect_target = r.len() == text.len();
        scan.pending_flag_value = false;
        return;
    }
    if scan.pending_redirect_target {
        scan.pending_redirect_target = false;
        return;
    }
    if is_flag(text) {
        scan.pending_flag_value = !text.contains('=') && text != "--";
        return;
    }
    if text == "-" || text == "--" {
        return;
    }
    let script_slot = SCRIPT_RUNNERS.contains(&bin)
        || scan
            .kept
            .last()
            .is_some_and(|k| RUN_WORDS.contains(&k.as_str()));
    if is_path_like(text) && !word.quoted {
        let base = script_base(text);
        if script_slot && !scan.saw_script && !base.is_empty() && !is_literal(&base) {
            scan.kept.push(base);
            scan.saw_script = true;
        }
        scan.saw_operand = true;
        scan.pending_flag_value = false;
        return;
    }
    if scan.pending_flag_value {
        scan.pending_flag_value = false;
        return;
    }
    if word.quoted || is_literal(text) || is_env_assign(text) {
        scan.saw_operand = true;
        return;
    }
    if scan.saw_operand {
        return;
    }
    scan.kept.push(text.to_owned());
    if script_slot {
        scan.saw_script = true;
    }
}

static FOR_LOOP: LazyRe = js_re!(r"^for\s+[A-Za-z_][A-Za-z0-9_]*\s+in\b");

/// The binary and family of a command line.
pub fn normalize_command(raw: &str) -> NormalizedCommand {
    let cmd = strip_heredocs(raw);
    let segs = split_segments(&cmd);
    let primary = meaningful_segment(&segs.segments);
    if FOR_LOOP.is_match(primary) {
        return NormalizedCommand {
            bin: "for".into(),
            family: "for-loop".into(),
        };
    }
    let all = tokenize(primary);
    let texts: Vec<&str> = all.iter().map(|w| w.text.as_str()).collect();
    let words = &all[command_index(&texts)..];
    let bin = words.first().map_or("", |w| bin_name(&w.text)).to_owned();
    let mut scan = FamilyScan::default();
    for word in words.iter().skip(1) {
        if scan.kept.len() >= FAMILY_BUDGET {
            break;
        }
        step_family(&mut scan, word, &bin);
    }
    let joined = std::iter::once(bin.as_str())
        .chain(scan.kept.iter().map(String::as_str))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let family = if joined.is_empty() {
        text::trim(raw)
            .split(text::is_js_whitespace)
            .next()
            .unwrap_or("")
            .to_owned()
    } else {
        joined
    };
    NormalizedCommand {
        bin,
        family: units::head(&family, FAMILY_MAX).into_owned(),
    }
}

static QUOTED: LazyRe = js_re!(r#"'[^']*'|"[^"]*"|`[^`]*`"#, "g");
static EXIT_CODE: LazyRe = js_re!(r"^Exit code (\d+)\b");
static TOOL_USE_ERROR: LazyRe = js_re!(r"<tool_use_error>([\s\S]*?)(?:<\/tool_use_error>|$)");
static DURATION: LazyRe = js_re!(
    r"\b\d+(?:\.\d+)?\s*(?:h|hr|hrs)\b\s*\d*(?:\.\d+)?\s*m?\b|\b\d+(?:\.\d+)?m\s+\d+(?:\.\d+)?s\b|\b\d+(?:\.\d+)?(?:ms|s)\b",
    "g"
);
static HOSTNAME: LazyRe = js_re!(
    r"\b(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.){2,}[a-z]{2,24}\b",
    "gi"
);
static URL: LazyRe = js_re!(r"[a-z][a-z0-9+.-]*:\/\/\S+", "gi");
static UUID: LazyRe = js_re!(
    r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b",
    "gi"
);
static PATHY: LazyRe = js_re!(r"(?:[A-Za-z]:)?(?:[\w.@~+-]*[\\/])+[\w.@+-]*", "g");
static HEX: LazyRe = js_re!(r"\b[0-9a-f]{7,}\b", "gi");
static NUMBER: LazyRe = js_re!(r"\b\d+(?:[.,]\d+)*\b", "g");
static PLURAL: LazyRe = js_re!(r"\bissues?\b", "gi");
static LEADING_BIN: LazyRe = js_re!(r"^([a-z][\w.-]*):\s");
static SENTENCE_END: LazyRe = js_re!(r"[.!?](?:\s|$)");
static CLAUSE_SPLIT: LazyRe = js_re!(r",\s+");
static SHELL_TOOL: LazyRe = js_re!(r"(?:^|_)(?:bash|shell|run_command|execute_command)", "i");
static VALIDATION_LIST: LazyRe =
    js_re!(r"^(.{6,80}?):\s+(?:[\w./#[\]-]{1,60}|<path>|<n>):\s+\S");

fn mask_literals(s: &str) -> String {
    let s = URL.replace_all_str(s, "<url>");
    let s = UUID.replace_all_str(&s, "<uuid>");
    let s = DURATION.replace_all_str(&s, "<dur>");
    let s = PATHY.replace_all_str(&s, "<path>");
    let s = HOSTNAME.replace_all_str(&s, "<host>");
    let s = HEX.replace_all_str(&s, "<hash>");
    let s = NUMBER.replace_all_str(&s, "<n>");
    PLURAL.replace_all_str(&s, "issue(s)")
}

fn mask_leading_bin(line: &str) -> String {
    match LEADING_BIN.captures(line) {
        Some(m) if m.get(1).is_some_and(|n| MESSAGE_BINS.contains(&n)) => {
            format!("<bin>: {}", &line[m.end()..])
        }
        _ => line.to_owned(),
    }
}

fn non_empty_lines(s: &str) -> Vec<&str> {
    s.split('\n').map(text::trim).filter(|l| !l.is_empty()).collect()
}

fn first_sentence(s: &str) -> &str {
    let line = non_empty_lines(s).first().copied().unwrap_or("");
    SENTENCE_END
        .find(line)
        .map_or(line, |r| &line[..=r.start])
}

/// `s.replace(/\s+/g, " ").trim()`.
pub fn collapse_whitespace(s: &str) -> String {
    text::trim(&units::collapse_ws(s)).to_owned()
}

fn collapse_clauses(s: &str) -> &str {
    let parts = CLAUSE_SPLIT.split(s);
    if parts.len() < 2 {
        return s;
    }
    let shapes: Vec<String> = parts
        .iter()
        .map(|p| QUOTED.replace_all_str(p, "<str>"))
        .collect();
    let repeat = &shapes[1];
    let repeats = !repeat.is_empty()
        && shapes[1..].iter().all(|x| x == repeat)
        && shapes[0].ends_with(repeat.as_str());
    if repeats { parts[0] } else { s }
}

fn finish_sig(body: &str, prefix: &str) -> String {
    let collapsed = collapse_whitespace(collapse_clauses(body));
    let joined = if prefix.is_empty() {
        collapsed
    } else {
        text::trim(&format!("{prefix} {collapsed}")).to_owned()
    };
    units::head(&joined, SIG_MAX).into_owned()
}

fn shell_signature(s: &str) -> String {
    let lines = non_empty_lines(s);
    let exit = lines.first().and_then(|l| EXIT_CODE.captures(l));
    let body = if exit.is_some() { &lines[1..] } else { &lines[..] };
    let tail = body.last().copied().unwrap_or("");
    let prefix = exit
        .and_then(|e| e.get(1))
        .map_or(String::new(), |n| format!("Exit code {n}"));
    finish_sig(&mask_literals(&mask_leading_bin(tail)), &prefix)
}

fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

/// The masked signature of a failed call's output, or `None` when it is empty.
pub fn error_signature(text_in: &str, tool: &str) -> Option<String> {
    let s = text::trim(text_in);
    if s.is_empty() {
        return None;
    }
    if let Some(w) = TOOL_USE_ERROR.captures(s) {
        return non_empty(finish_sig(
            &mask_literals(first_sentence(w.get(1).unwrap_or(""))),
            "",
        ));
    }
    if EXIT_CODE.is_match(s) || SHELL_TOOL.is_match(tool) {
        return non_empty(shell_signature(s));
    }
    let head = non_empty_lines(s)
        .into_iter()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    non_empty(finish_sig(&mask_literals(&head), ""))
}

/// The cluster a signature belongs to.
pub fn cluster_key_of(sig: &str, redact: &dyn Fn(&str) -> String) -> String {
    let masked = collapse_whitespace(&QUOTED.replace_all_str(sig, "<str>"));
    if EXIT_CODE.is_match(&masked) {
        return units::head(&redact(&masked), SIG_MAX).into_owned();
    }
    let key = match VALIDATION_LIST.captures(&masked).and_then(|m| m.get(1)) {
        Some(head) if !head.is_empty() => redact(&format!("{head}:")),
        _ => redact(&masked),
    };
    units::head(&key, SIG_MAX).into_owned()
}

/// The quoted literals of a signature, quotes removed, empty ones dropped.
pub fn details_of(sig: &str) -> Vec<String> {
    QUOTED
        .captures_all(sig)
        .iter()
        .map(|c| {
            let q = c.all();
            q[1..q.len() - 1].to_owned()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families() {
        let f = |s: &str| normalize_command(s).family;
        assert_eq!(f("bun run typecheck"), "bun run typecheck");
        assert_eq!(f("cd x && git status --short"), "git status");
        assert_eq!(f("for f in *; do echo $f; done"), "for-loop");
        assert_eq!(f("python3 scripts/build.py --fast"), "python3 build");
    }

    #[test]
    fn signatures() {
        assert_eq!(
            error_signature("Exit code 1\nls: /tmp/x: No such file", "Bash").as_deref(),
            Some("Exit code 1 <bin>: <path>: No such file")
        );
        assert_eq!(
            error_signature("<tool_use_error>File does not exist. Try again.</tool_use_error>", "Read")
                .as_deref(),
            Some("File does not exist.")
        );
    }
}
