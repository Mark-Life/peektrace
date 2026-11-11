//! Retry pairing: each failed call is matched to the call that corrected
//! it, within a few calls, a time gap and a similarity floor.

use std::collections::HashSet;

use indexmap::IndexMap;

use super::causes::{FILE_EDIT_TOOLS, spec};
use super::failures::USER_REJECTED;
use super::similarity::{FocusMode, SimilarityInput, hybrid_similarity, ratio, segments};
use super::waiting::wait_shape;
use crate::js::regex::LazyRe;
use crate::js::text;
use crate::js::time::{LocalZone, parse_date};
use crate::js::units;
use crate::js_re;
use crate::stats::schema::{CallFact, CauseId, WaitShape};
use crate::stats::shell_lex::{Ctx, bin_name, command_index, shell_contexts};

pub const RETRY_WINDOW: f64 = 3.0;
pub const PAIR_THRESHOLD: f64 = 0.6;
pub const MAX_RETRY_GAP_SEC: f64 = 600.0;
const DIFF_MAX: usize = 80;
const CLIP_MARK: &str = "…";
const REWRITE_MIN: f64 = 0.5;

/// The token change a retry made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedDiff {
    pub from: String,
    pub to: String,
}

/// One failed call and the call that corrected it.
#[derive(Debug, Clone)]
pub struct RetryPair {
    pub cause: CauseId,
    pub diff: Option<LearnedDiff>,
    pub failed: CallFact,
    pub gap_sec: Option<f64>,
    pub read_back: bool,
    pub retry: CallFact,
}

fn seed_cause_of(f: &CallFact) -> Option<CauseId> {
    if let Some(c) = f.cause {
        return spec(c).failing.then_some(c);
    }
    if !f.err || f.benign.is_some() || f.fail_reason.is_some() {
        return None;
    }
    if f.sig.as_deref().is_some_and(|s| USER_REJECTED.is_match(s)) {
        None
    } else {
        Some(CauseId::Unclassified)
    }
}

/// The cause a call is counted under in the cause table, if any.
pub fn hit_cause_of(f: &CallFact) -> Option<CauseId> {
    if f.cause == Some(CauseId::OutputTooLarge) {
        f.cause
    } else {
        seed_cause_of(f)
    }
}

fn mode_of(cause: CauseId) -> FocusMode {
    match cause {
        CauseId::ZshGlobFlag | CauseId::ZshGlobPath => FocusMode::Glob,
        CauseId::CommandNotFound => FocusMode::Cnf,
        CauseId::SleepBlocked => FocusMode::Sleep,
        _ => FocusMode::Whole,
    }
}

fn text_of(f: &CallFact) -> &str {
    match &f.bash {
        Some(b) => &b.cmd,
        None => f.arg.as_deref().unwrap_or(""),
    }
}

fn are_siblings(a: &CallFact, b: &CallFact) -> bool {
    match (&a.req, &b.req) {
        (Some(x), Some(y)) => x == y,
        _ => a.ts.is_some() && a.ts == b.ts,
    }
}

/// Words split at code or heredoc whitespace and at `;&|`.
fn quoted_words(cmd: &str) -> Vec<String> {
    let ctx = shell_contexts(cmd);
    let mut words = Vec::new();
    let mut buf = String::new();
    for (i, c) in cmd.char_indices() {
        let splits = matches!(ctx[i], Ctx::Code | Ctx::Heredoc);
        if splits && (text::is_js_whitespace(c) || matches!(c, ';' | '&' | '|')) {
            if !buf.is_empty() {
                words.push(std::mem::take(&mut buf));
            }
        } else {
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        words.push(buf);
    }
    words
}

fn unclipped_words(cmd: &str) -> Vec<String> {
    let mut w = quoted_words(cmd);
    if cmd.ends_with(CLIP_MARK) {
        w.pop();
    }
    w
}

fn word_diff(a: &[String], b: &[String]) -> (Vec<String>, Vec<String>) {
    let mut t = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            t[i][j] = if a[i] == b[j] {
                t[i + 1][j + 1] + 1
            } else {
                t[i + 1][j].max(t[i][j + 1])
            };
        }
    }
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
        } else if t[i + 1][j] >= t[i][j + 1] {
            removed.push(a[i].clone());
            i += 1;
        } else {
            added.push(b[j].clone());
            j += 1;
        }
    }
    removed.extend_from_slice(&a[i..]);
    added.extend_from_slice(&b[j..]);
    (removed, added)
}

fn clip_diff(s: &str) -> String {
    units::clip(s, DIFF_MAX)
}

fn diff_of_sides(from: &str, to: &str) -> Option<LearnedDiff> {
    let (a, b) = (clip_diff(from), clip_diff(to));
    (a != b).then_some(LearnedDiff { from: a, to: b })
}

fn rewrite_of<'a>(from: &str, added: &'a [String]) -> Option<(&'a str, f64)> {
    let mut found: Option<(&str, f64)> = None;
    for w in added {
        let score = ratio(from, w);
        if score >= REWRITE_MIN && found.is_none_or(|f| score > f.1) {
            found = Some((w, score));
        }
    }
    found
}

type Culprit<'a> = Box<dyn Fn(&str) -> bool + 'a>;

static EQUALS_WORD: LazyRe = js_re!(r#"^["']?="#);

fn culprit_of(cause: CauseId, arg: Option<&str>) -> Option<Culprit<'_>> {
    match cause {
        CauseId::ZshEqualsAbort => Some(Box::new(|w: &str| EQUALS_WORD.is_match(w))),
        CauseId::PkillSelfKill => Some(Box::new(|w: &str| matches!(w, "pkill" | "pgrep" | "kill"))),
        _ => arg.map(|a| Box::new(move |w: &str| w.contains(a)) as Culprit<'_>),
    }
}

fn learned_diff(failed_cmd: &str, retry_cmd: &str, culprit: Option<Culprit<'_>>) -> Option<LearnedDiff> {
    let (rem, add) = word_diff(&unclipped_words(failed_cmd), &unclipped_words(retry_cmd));
    let moved: HashSet<&String> = rem.iter().filter(|w| add.contains(w)).collect();
    let removed: Vec<String> = rem.iter().filter(|w| !moved.contains(w)).cloned().collect();
    let added: Vec<String> = add.iter().filter(|w| !moved.contains(w)).cloned().collect();
    if let Some(pick) = culprit {
        let c = removed.iter().find(|w| pick(w))?;
        return diff_of_sides(c, rewrite_of(c, &added).map_or("(dropped)", |r| r.0));
    }
    let mut best: Option<(&str, &str, f64)> = None;
    for from in &removed {
        if let Some((to, score)) = rewrite_of(from, &added) {
            if best.is_none_or(|b| score > b.2) {
                best = Some((from, to, score));
            }
        }
    }
    best.and_then(|(f, t, _)| diff_of_sides(f, t))
}

static OPENER: LazyRe = js_re!(r"^(?:\$\(|[({`])+");
const LEAD_WORDS: [&str; 7] = ["!", "do", "else", "if", "then", "until", "while"];
const BUILTINS: [&str; 16] = [
    ".", "alias", "cd", "eval", "exit", "export", "false", "hash", "popd", "pushd", "rehash",
    "return", "set", "source", "true", "unset",
];

fn command_word_of(s: &str) -> Option<String> {
    let words: Vec<String> = s
        .split(' ')
        .map(|raw| OPENER.replace_first(raw, |_| String::new()))
        .filter(|w| !w.is_empty())
        .collect();
    let lead = words.iter().position(|w| !LEAD_WORDS.contains(&w.as_str()));
    let rest = lead.map_or(&[][..], |l| &words[l..]);
    let word = rest.get(command_index(rest))?;
    let name = bin_name(word);
    (!BUILTINS.contains(&name)).then(|| name.to_owned())
}

static SLEEP_WORD: LazyRe = js_re!(r"\bsleep\s+\.?\d+(?:\.\d+)?");
static SHELL_PAYLOAD: LazyRe = js_re!(r#"\b(?:ba|z)?sh\s+-\w*c\s+(['"])([\s\S]*?)\1"#, "g");

fn wait_in(cmd: &str, wait: Option<WaitShape>) -> Option<WaitShape> {
    wait.or_else(|| {
        SHELL_PAYLOAD
            .captures_all(cmd)
            .iter()
            .find_map(|m| wait_shape(m.get(2).unwrap_or("")))
    })
}

fn wait_of(retry: &CallFact) -> String {
    let Some(b) = &retry.bash else { return "(no wait)".into() };
    if b.bg {
        return "run_in_background".into();
    }
    match wait_in(&b.cmd, b.wait) {
        Some(WaitShape::LoopPoll) => "poll loop".into(),
        Some(WaitShape::Watcher) => "watcher".into(),
        Some(WaitShape::CurlRetry) => "curl --retry".into(),
        Some(WaitShape::BareDelay) => SLEEP_WORD
            .find(&b.cmd)
            .map_or_else(|| "sleep".to_owned(), |r| b.cmd[r].to_owned()),
        None => "(no wait)".into(),
    }
}

#[derive(Debug, Clone)]
struct Best {
    retry: CallFact,
    score: f64,
    segment: String,
}

#[derive(Debug, Clone)]
struct Seed {
    best: Option<Best>,
    cause: CauseId,
    edit_seen: bool,
    fact: CallFact,
    input: SimilarityInput,
    read_back: bool,
    seen: f64,
}

fn diff_of(seed: &Seed, retry: &CallFact, segment: &str) -> Option<LearnedDiff> {
    let (Some(fb), Some(rb)) = (&seed.fact.bash, &retry.bash) else { return None };
    let arg = seed.fact.cause_arg.as_deref();
    match seed.cause {
        CauseId::CommandNotFound | CauseId::SleepBlocked if arg.is_none() => None,
        CauseId::SleepBlocked => diff_of_sides(arg.unwrap_or(""), &wait_of(retry)),
        CauseId::CommandNotFound => {
            let ran = seed.input.focus.as_ref().and_then(|_| command_word_of(segment))?;
            diff_of_sides(arg.unwrap_or(""), &ran)
        }
        cause => learned_diff(&fb.cmd, &rb.cmd, culprit_of(cause, arg)),
    }
}

static SHELL_EDITS: [LazyRe; 5] = [
    js_re!(r"\b(?:g?sed|perl)\s(?:[^|;&\n]*\s)?(?:-[A-Za-z]*i|--in-place)\b"),
    js_re!(r#"\bopen\([^)]*,\s*(?:mode\s*=\s*)?["'][wax]"#),
    js_re!(r"\.write_(?:text|bytes)\(|\bwriteFile(?:Sync)?\(|\bBun\.write\("),
    js_re!(r"\bcat\b[^|;&\n]*>\s*(?!\/dev\/)[^\s&>]"),
    js_re!(r"\btee\s+(?:-a\s+)?(?!\/dev\/|-)\S"),
];

fn shell_line_of(f: &CallFact) -> Option<&str> {
    match &f.bash {
        Some(b) => Some(&b.cmd),
        None if f.tool == "exec_command" => f.arg.as_deref(),
        None => None,
    }
}

fn edits_file(f: &CallFact) -> bool {
    if FILE_EDIT_TOOLS.contains(&f.tool.as_str()) || f.bash.as_ref().is_some_and(|b| b.bin == "apply_patch") {
        return true;
    }
    shell_line_of(f).is_some_and(|l| SHELL_EDITS.iter().any(|re| re.is_match(l)))
}

#[derive(Clone, Copy)]
struct Clock<'a>(&'a dyn LocalZone);

impl Clock<'_> {
    #[expect(clippy::cast_precision_loss)]
    fn ms(self, f: &CallFact) -> Option<f64> {
        parse_date(f.ts.as_deref()?, self.0).map(|t| t as f64)
    }

    fn gap(self, a: &CallFact, b: &CallFact) -> Option<f64> {
        Some((self.ms(b)? - self.ms(a)?) / 1000.0)
    }
}

fn in_reach(seed: &Seed, retry: &CallFact, clock: Clock<'_>) -> bool {
    let same_tool = retry.tool == seed.fact.tool || (seed.cause == CauseId::CommandNotFound && retry.is_bash());
    if !same_tool || are_siblings(&seed.fact, retry) {
        return false;
    }
    if clock.gap(&seed.fact, retry).is_some_and(|g| !(0.0..=MAX_RETRY_GAP_SEC).contains(&g)) {
        return false;
    }
    !(seed.cause == CauseId::Unclassified && seed.edit_seen)
}

fn same_call(a: &CallFact, b: &CallFact) -> bool {
    a.base.sid == b.base.sid && a.seq == b.seq
}

fn reads_back(seed: &Seed, f: &CallFact) -> bool {
    let Some(name) = seed.fact.cause_arg.as_deref() else { return false };
    if same_call(&seed.fact, f) || are_siblings(&seed.fact, f) {
        return false;
    }
    f.arg.as_deref().is_some_and(|a| a.contains(name)) || f.bash.as_ref().is_some_and(|b| b.cmd.contains(name))
}

fn close_seed(seed: Seed, clock: Clock<'_>, out: &mut Vec<RetryPair>) {
    let Some(best) = seed.best.as_ref() else { return };
    let diff = if seed.read_back {
        None
    } else {
        diff_of(&seed, &best.retry, &best.segment)
    };
    out.push(RetryPair {
        cause: seed.cause,
        gap_sec: clock.gap(&seed.fact, &best.retry),
        read_back: seed.read_back,
        diff,
        retry: best.retry.clone(),
        failed: seed.fact,
    });
}

enum Verdict {
    Skip,
    Chain,
    Score(f64, String),
}

fn verdict_of(seed: &Seed, f: &CallFact, split: &dyn Fn(&str) -> Vec<String>, clock: Clock<'_>) -> Verdict {
    if !in_reach(seed, f, clock) {
        return Verdict::Skip;
    }
    let sim = hybrid_similarity(&seed.input, text_of(f), PAIR_THRESHOLD, split);
    if sim.score < PAIR_THRESHOLD {
        Verdict::Skip
    } else if f.failed || f.err {
        Verdict::Chain
    } else {
        Verdict::Score(sim.score, sim.segment)
    }
}

/// Offer a call to a lane's seeds; returns the indexes the call chained.
fn offer_to(seeds: &mut [Seed], f: &CallFact, clock: Clock<'_>) -> HashSet<usize> {
    for seed in seeds.iter_mut() {
        if seed.read_back && seed.best.is_none() && reads_back(seed, f) {
            seed.best = Some(Best {
                retry: f.clone(),
                score: 1.0,
                segment: String::new(),
            });
        }
    }
    let mut closed = HashSet::new();
    let cache: std::cell::OnceCell<Vec<String>> = std::cell::OnceCell::new();
    let split = |cmd: &str| cache.get_or_init(|| segments(cmd)).clone();
    let mut open: Option<(usize, f64, String)> = None;
    let mut better: Option<(usize, f64, String)> = None;
    for (i, seed) in seeds.iter().enumerate().rev() {
        if seed.read_back || open.is_some() {
            continue;
        }
        match verdict_of(seed, f, &split, clock) {
            Verdict::Chain => {
                closed.insert(i);
            }
            Verdict::Skip => {}
            Verdict::Score(score, segment) => match &seed.best {
                None => open = Some((i, score, segment)),
                Some(b) if better.is_none() && score > b.score => better = Some((i, score, segment)),
                Some(_) => {}
            },
        }
    }
    if let Some((i, score, segment)) = open.or(better) {
        seeds[i].best = Some(Best {
            retry: f.clone(),
            score,
            segment,
        });
    }
    closed
}

fn seed_of(f: &CallFact) -> Option<Seed> {
    let cause = hit_cause_of(f)?;
    let read_back = cause == CauseId::OutputTooLarge;
    if read_back && f.cause_arg.is_none() {
        return None;
    }
    let text = text_of(f);
    if text.is_empty() && !(read_back || cause == CauseId::StructuredOutputSchema) {
        return None;
    }
    Some(Seed {
        input: SimilarityInput {
            text: text.to_owned(),
            focus: f.focus.clone(),
            mode: mode_of(cause),
            shell: f.is_bash(),
        },
        fact: f.clone(),
        cause,
        read_back,
        best: None,
        edit_seen: false,
        seen: 0.0,
    })
}

/// Folds call facts into retry pairs, one session at a time.
pub struct Pairer<'a> {
    sid: Option<String>,
    lanes: IndexMap<bool, Vec<Seed>>,
    clock: Clock<'a>,
}

impl<'a> Pairer<'a> {
    pub fn new(local: &'a dyn LocalZone) -> Self {
        Self {
            sid: None,
            lanes: IndexMap::new(),
            clock: Clock(local),
        }
    }

    /// Close every open seed, returning the pairs they hold.
    pub fn flush(&mut self) -> Vec<RetryPair> {
        let mut out = Vec::new();
        for (_, seeds) in self.lanes.drain(..) {
            for seed in seeds {
                close_seed(seed, self.clock, &mut out);
            }
        }
        out
    }

    /// Fold one call; facts of a session arrive together, in seq order.
    pub fn step(&mut self, f: &CallFact) -> Vec<RetryPair> {
        let mut out = Vec::new();
        if self.sid.as_deref() != Some(f.base.sid.as_str()) {
            out = self.flush();
            self.sid = Some(f.base.sid.clone());
        }
        let clock = self.clock;
        let seeds = self.lanes.entry(f.side).or_default();
        let closed = offer_to(seeds, f, clock);
        let edit = edits_file(f);
        let mut kept = Vec::new();
        for (i, mut seed) in std::mem::take(seeds).into_iter().enumerate() {
            if !are_siblings(&seed.fact, f) {
                seed.seen += 1.0;
            }
            seed.edit_seen |= edit;
            if closed.contains(&i) || seed.seen >= RETRY_WINDOW {
                close_seed(seed, clock, &mut out);
            } else {
                kept.push(seed);
            }
        }
        kept.extend(seed_of(f));
        *seeds = kept;
        out
    }
}
