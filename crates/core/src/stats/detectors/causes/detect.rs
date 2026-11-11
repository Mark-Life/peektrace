//! Per-call cause detection: reads the command and its output while both are
//! in hand at extract time.

use super::catalog::{WRITE_TOOLS, is_shell_tool};
use crate::js::regex::{LazyRe, Re};
use crate::js::text;
use crate::js_re;
use crate::stats::detectors::similarity::{FOCUS_SEP, norm_seg, segments};
use crate::stats::rank::collapse_ws;
use crate::stats::schema::CauseId;
use crate::stats::shell_lex::{UNARY_WRAPPERS, WRAPPER_OPERAND, unescaped, unquoted_at};

/// What one detector found on one call. Strings are raw (unredacted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CauseHit {
    pub cause: CauseId,
    pub arg: Option<String>,
    pub focus: Option<String>,
    pub line: Option<String>,
}

fn hit(cause: CauseId) -> CauseHit {
    CauseHit {
        cause,
        arg: None,
        focus: None,
        line: None,
    }
}

static GLOB_LINE: LazyRe = js_re!(r"^(?:\(eval\)|zsh):\d+: no matches found: (\S+)", "gm");
static CNF_LINE: LazyRe = js_re!(
    r"^(?:\(eval\)|(?:\/\S*\/)?(?:zsh|bash))(?::\d+)?:(?: line \d+:)? (?:command not found: (\S+)|(\S+): command not found)",
    "gm"
);
static EQUALS_LINE: LazyRe = js_re!(r"^(?:\(eval\)|zsh):\d+: (=\S*) not found$", "gm");
static EXIT_144: LazyRe = js_re!(r"^Exit code 144\b");
static PKILL_SELF: LazyRe = js_re!(
    r"\bpkill\b[^;&|\n]*?\s(?:-\w*f|--full)\b|\bpgrep\b[^;&|\n]*?\s(?:-\w*f|--full)\b[^;\n]*?(?:\|\s*xargs\s+(?:-\S+\s+)*kill\b|\)[^\n]*\bkill\b)|\bkill\s+(?:-\S+\s+)*\$\((?:pgrep|ps)\b|\bps\b[^;&|\n]*\|\s*grep\b[^;\n]*\|[^;\n]*\bkill\b"
);
static SLEEP_BLOCKED: LazyRe =
    js_re!(r"^Blocked: sleep (\d+) followed by:? ?([\s\S]*?)(?:\. To wait|$)");
static PERSISTED: LazyRe = js_re!(r"^<persisted-output>\nOutput too large");
static PERSISTED_PATH: LazyRe = js_re!(r"Full output saved to: (\S+)");
static NOT_READ: LazyRe = js_re!(r"^File has not been read yet");
static STALE_READ: LazyRe = js_re!(r"^File has been modified since read");
static EDIT_NO_MATCH: LazyRe =
    js_re!(r"^(?:String to replace not found|Found \d+ matches of the string)");
static SCHEMA_FAIL: LazyRe = js_re!(
    r"does not match required schema|could not be parsed as JSON|must NOT have additional properties"
);
static SCHEMA_FIELD: LazyRe = js_re!(r"required property '(\w+)'");
static RM_GUARD: LazyRe = js_re!(
    r"^(?:Dangerous rm operation detected|Permission for this command was denied by a built-in Claude Code safety check[\s\S]{0,200}removals)"
);
static SCRATCH_MODULE: [LazyRe; 3] = [
    js_re!(
        r"^(?:error: )?Cannot find (?:module|package) '[^']+' from '([^']+)'",
        "m"
    ),
    js_re!(
        r"^Error \[ERR_MODULE_NOT_FOUND\]: Cannot find (?:module|package) '[^']+' imported from (?:file:\/\/)?(.+)$",
        "m"
    ),
    js_re!(
        r"^Error: Cannot find module '[^']+'\r?\nRequire stack:\r?\n- (.+)$",
        "m"
    ),
];
static SCRATCH_DIR: LazyRe = js_re!(
    r"^(?:[^/]*[-_.])?(?:tmp|temp|scratch)(?:[-_.][^/]*|\d*)$",
    "i"
);
const SCRIPT_RUNTIMES: [&str; 5] = ["bun", "bunx", "node", "tsx", "npx"];
static UGREP: LazyRe = js_re!(
    r"^ugrep: error: (?:error at position|.*exceeds complexity)",
    "m"
);
static GREP_WORD: LazyRe = js_re!(r"\b(?:u?grep|egrep)\b");
static EXIT_HEADER: LazyRe = js_re!(r"^Exit code \d+\n?");
static TOOL_USE_ERROR_OPEN: LazyRe = js_re!(r"^<tool_use_error>");
static GLOB_CHAR: LazyRe = js_re!(r"[*?[]");
static VERSION_ARGS: LazyRe =
    js_re!(r"^\s+(?:--version|-v|-V|version)(?:\s+[\d&]*>>?&?\s*\S+)*\s*$");
static COMMAND_V: LazyRe = js_re!(r"\bcommand\s+(?:-\S+\s+)*-[vV]\b");

const GLOB_FLAGS: [&str; 11] = [
    "-g",
    "--glob",
    "--include",
    "--exclude",
    "--exclude-dir",
    "--include-dir",
    "-name",
    "-iname",
    "-path",
    "-ipath",
    "-wholename",
];
const WORD_EDGE: &str = r"(?:^|[\s;&|()<>`=])";
const WORD_END: &str = r"(?=$|[\s;&|()<>`])";
const EXPANDED_HEAD: &str = r"(?:~[\w.-]*|\$\{?\w+\}?)";

/// Escape a string as a literal inside a JS regex.
pub fn escape_re(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if ".*+?^${}()|[]\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// First match whose group 1 starts in code.
fn unquoted_word(cmd: &str, re: &Re) -> Option<String> {
    re.captures_all(cmd).into_iter().find_map(|m| {
        let r = m.range(1).filter(|r| !r.is_empty())?;
        unquoted_at(cmd, r.start).then(|| cmd[r].to_owned())
    })
}

/// The culprit as typed with backslash escapes, mapped back from the unescaped text.
fn escaped_spelling(cmd: &str, exact: &Re) -> Option<String> {
    if !cmd.contains('\\') {
        return None;
    }
    let u = unescaped(cmd);
    exact.captures_all(&u.text).into_iter().find_map(|m| {
        let r = m.range(1).filter(|r| !r.is_empty())?;
        let start = *u.from.get(r.start)?;
        let last = *u.from.get(r.end - 1)?;
        let end = (last + 1..=cmd.len()).find(|&e| cmd.is_char_boundary(e))?;
        unquoted_at(cmd, start).then(|| cmd[start..end].to_owned())
    })
}

/// How the culprit zsh named is spelled in the command, or `None` when the
/// command does not hold it at a code position.
fn command_glob(cmd: &str, word: &str) -> Option<String> {
    let exact = Re::new(&format!("{WORD_EDGE}({}){WORD_END}", escape_re(word)), "g");
    if cmd.contains(word) && unquoted_word(cmd, &exact).is_some() {
        return Some(word.to_owned());
    }
    if let Some(e) = escaped_spelling(cmd, &exact) {
        return Some(e);
    }
    let at = GLOB_CHAR.search(word).filter(|&a| a > 0);
    if let Some(at) = at {
        let head = &word[..=at];
        if cmd.contains(head) {
            let re = Re::new(
                &format!(r"{WORD_EDGE}({}[^\s;&|()<>`]*)", escape_re(head)),
                "g",
            );
            if unquoted_word(cmd, &re).is_some_and(|b| b.contains('{')) {
                return Some(word.to_owned());
            }
        }
    }
    let at = GLOB_CHAR.search(word)?;
    let mut slash = word.find('/');
    while let Some(s) = slash.filter(|&s| s < at) {
        let tail = &word[s..];
        if cmd.contains(tail) {
            let re = Re::new(
                &format!("{WORD_EDGE}({EXPANDED_HEAD}{}){WORD_END}", escape_re(tail)),
                "",
            );
            if let Some(m) = re
                .captures(cmd)
                .and_then(|c| c.get(1))
                .filter(|g| !g.is_empty())
            {
                return Some(m.to_owned());
            }
        }
        slash = word[s + 1..].find('/').map(|n| n + s + 1);
    }
    None
}

fn preceding_token<'a>(cmd: &'a str, word: &str) -> &'a str {
    let head = cmd.find(word).map_or("", |at| &cmd[..at]);
    text::trim_end(head)
        .rsplit(text::is_js_whitespace)
        .next()
        .unwrap_or("")
}

fn is_flag_glob(cmd: &str, word: &str) -> bool {
    word.starts_with('-') || GLOB_FLAGS.contains(&preceding_token(cmd, word))
}

fn wrapper() -> String {
    format!(
        r"(?:(?:{})\s+(?:(?:-\S+|{WRAPPER_OPERAND})\s+)*)*",
        UNARY_WRAPPERS.join("|")
    )
}

/// `bin` at a command position; group 1 is the wrapper run, group 2 the args.
pub fn bin_use(bin: &str, flags: &str) -> Re {
    let env_prefix = r"(?:[A-Za-z_][A-Za-z0-9_]*=\S*\s+)*";
    Re::new(
        &format!(
            r"(?:^|[;&|(`\n{{]|\$\(|\b(?:then|do|else)\s)\s*{env_prefix}({})(?:\S*/)?{}(?=$|[\s;&|)`])((?:>&|&>|[^;&|\n)`])*)",
            wrapper(),
            escape_re(bin)
        ),
        flags,
    )
}

fn runs_for_real(cmd: &str, bin: &str) -> bool {
    bin_use(bin, "g").captures_all(cmd).iter().any(|m| {
        !(COMMAND_V.is_match(m.get(1).unwrap_or(""))
            || VERSION_ARGS.is_match(m.get(2).unwrap_or("")))
    })
}

fn join_focus(segs: &[String]) -> Option<String> {
    (!segs.is_empty()).then(|| segs.join(FOCUS_SEP))
}

fn focus_segments(cmd: &str) -> Vec<String> {
    segments(&collapse_ws(cmd))
}

/// Hides secrets in raw text before a focus is cut from it.
pub type Scrub<'a> = &'a dyn Fn(&str) -> String;

fn glob_focus(cmd: &str, word: &str, scrub: Scrub<'_>) -> Option<String> {
    let w = norm_seg(word);
    let kept: Vec<String> = focus_segments(&scrub(cmd))
        .into_iter()
        .filter(|s| s.contains(&w))
        .collect();
    join_focus(&kept)
}

fn cnf_focus(cmd: &str, bin: &str, scrub: Scrub<'_>) -> Option<String> {
    let suffix = format!("/{bin}");
    let kept: Vec<String> = focus_segments(&scrub(cmd))
        .iter()
        .filter_map(|s| {
            let words: Vec<&str> = s.split(' ').collect();
            let at = words
                .iter()
                .position(|w| *w == bin || w.ends_with(&suffix))?;
            Some(words[at + 1..].join(" "))
        })
        .collect();
    join_focus(&kept)
}

/// The output without Claude's exit header and a harness error tag.
pub fn harness_body(output: &str) -> String {
    let s = EXIT_HEADER.replace_first(output, |_| String::new());
    TOOL_USE_ERROR_OPEN.replace_first(&s, |_| String::new())
}

struct ShellInput<'a> {
    cmd: &'a str,
    output: &'a str,
    scrub: Scrub<'a>,
}

fn detect_glob(i: &ShellInput<'_>) -> Option<CauseHit> {
    if !i.output.contains("no matches found") {
        return None;
    }
    GLOB_LINE.captures_all(i.output).into_iter().find_map(|m| {
        let culprit = m.get(1).filter(|c| !c.is_empty())?;
        let word = command_glob(i.cmd, culprit)?;
        let cause = if is_flag_glob(i.cmd, &word) {
            CauseId::ZshGlobFlag
        } else {
            CauseId::ZshGlobPath
        };
        Some(CauseHit {
            cause,
            focus: glob_focus(i.cmd, &word, i.scrub),
            arg: Some(word),
            line: Some(m.all().to_owned()),
        })
    })
}

fn detect_equals(i: &ShellInput<'_>) -> Option<CauseHit> {
    if !i.output.contains(": =") {
        return None;
    }
    EQUALS_LINE
        .captures_all(i.output)
        .into_iter()
        .find_map(|m| {
            let word = format!("={}", m.get(1).unwrap_or(""));
            Re::new(&format!("{WORD_EDGE}{}", escape_re(&word)), "")
                .is_match(i.cmd)
                .then(|| CauseHit {
                    arg: Some(word),
                    line: Some(m.all().to_owned()),
                    ..hit(CauseId::ZshEqualsAbort)
                })
        })
}

fn detect_cnf(i: &ShellInput<'_>) -> Option<CauseHit> {
    if !i.output.contains("command not found") {
        return None;
    }
    CNF_LINE.captures_all(i.output).into_iter().find_map(|m| {
        let bin = m.get(1).or(m.get(2)).filter(|b| !b.is_empty())?;
        runs_for_real(i.cmd, bin).then(|| CauseHit {
            arg: Some(bin.to_owned()),
            focus: cnf_focus(i.cmd, bin, i.scrub),
            line: Some(m.all().to_owned()),
            ..hit(CauseId::CommandNotFound)
        })
    })
}

fn detect_sleep(body: &str, scrub: Scrub<'_>) -> Option<CauseHit> {
    let m = SLEEP_BLOCKED.captures(body)?;
    let n = m.get(1).unwrap_or("");
    let rest = norm_seg(&scrub(m.get(2).unwrap_or("")));
    Some(CauseHit {
        arg: Some(format!("sleep {n}")),
        focus: (!rest.is_empty()).then_some(rest),
        line: Some(format!("Blocked: sleep {n}")),
        ..hit(CauseId::SleepBlocked)
    })
}

fn is_scratch_path(path: &str) -> bool {
    if path.contains("/var/folders/") {
        return true;
    }
    let parts: Vec<&str> = path.split(['/', '\\']).collect();
    parts[..parts.len().saturating_sub(1)]
        .iter()
        .any(|d| SCRATCH_DIR.is_match(d))
}

fn detect_scratch(i: &ShellInput<'_>) -> Option<CauseHit> {
    if !i.output.contains("Cannot find ") {
        return None;
    }
    let m = SCRATCH_MODULE.iter().find_map(|re| re.captures(i.output))?;
    let ok = is_scratch_path(m.get(1).unwrap_or(""))
        && SCRIPT_RUNTIMES
            .iter()
            .any(|b| bin_use(b, "").is_match(i.cmd));
    ok.then(|| CauseHit {
        line: Some(m.all().split('\n').next().unwrap_or("").to_owned()),
        ..hit(CauseId::ScratchModuleResolution)
    })
}

fn detect_persisted(output: &str) -> Option<CauseHit> {
    if !(output.starts_with("<persisted-output>") && PERSISTED.is_match(output)) {
        return None;
    }
    let path = PERSISTED_PATH.captures(output).and_then(|c| c.get(1));
    Some(CauseHit {
        arg: path.map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p).to_owned()),
        ..hit(CauseId::OutputTooLarge)
    })
}

fn detect_shell(i: &ShellInput<'_>) -> Option<CauseHit> {
    if let Some(h) = detect_glob(i) {
        return Some(h);
    }
    if EXIT_144.is_match(i.output) && PKILL_SELF.is_match(i.cmd) {
        return Some(CauseHit {
            line: Some("Exit code 144".into()),
            ..hit(CauseId::PkillSelfKill)
        });
    }
    let body = harness_body(i.output);
    if body.starts_with("Blocked: sleep") {
        return detect_sleep(&body, i.scrub);
    }
    if let Some(h) = detect_cnf(i).or_else(|| detect_equals(i)) {
        return Some(h);
    }
    if RM_GUARD.is_match(&body) {
        return Some(CauseHit {
            line: Some(body.split('\n').next().unwrap_or("").to_owned()),
            ..hit(CauseId::RmGuard)
        });
    }
    if let Some(h) = detect_scratch(i) {
        return Some(h);
    }
    if i.output.contains("ugrep: error")
        && GREP_WORD.is_match(i.cmd)
        && let Some(m) = UGREP.captures(i.output)
    {
        return Some(CauseHit {
            line: Some(m.all().to_owned()),
            ..hit(CauseId::UgrepComplexity)
        });
    }
    detect_persisted(i.output)
}

fn detect_write(tool: &str, body: &str) -> Option<CauseHit> {
    if NOT_READ.is_match(body) {
        return Some(hit(CauseId::ReadBeforeWrite));
    }
    if STALE_READ.is_match(body) {
        return Some(hit(CauseId::StaleRead));
    }
    ((tool == "Edit" || tool == "MultiEdit") && EDIT_NO_MATCH.is_match(body))
        .then(|| hit(CauseId::EditNoMatch))
}

fn detect_schema(output: &str) -> Option<CauseHit> {
    SCHEMA_FAIL.is_match(output).then(|| CauseHit {
        arg: SCHEMA_FIELD
            .captures(output)
            .and_then(|c| c.get(1))
            .map(str::to_owned),
        ..hit(CauseId::StructuredOutputSchema)
    })
}

/// The named cause of one call, if any.
pub fn detect_cause(tool: &str, cmd: &str, output: &str, scrub: Scrub<'_>) -> Option<CauseHit> {
    if is_shell_tool(tool) {
        return detect_shell(&ShellInput { cmd, output, scrub });
    }
    if WRITE_TOOLS.contains(&tool) {
        return detect_write(tool, &harness_body(output));
    }
    if tool == "StructuredOutput" {
        return detect_schema(output);
    }
    None
}

/// The glob a culprit passes: a flag value loses its `--flag=`.
pub fn glob_value(word: &str) -> &str {
    match word.find('=') {
        Some(eq) if word.starts_with('-') && eq > 0 => &word[eq + 1..],
        _ => word,
    }
}
