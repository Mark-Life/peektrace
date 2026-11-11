//! Retry pairing: each failed call is matched to the call that corrected
//! it, within a few calls, a time gap and a similarity floor.

use std::collections::HashSet;

use indexmap::IndexMap;

use super::causes::{FILE_EDIT_TOOLS, spec};
use super::failures::USER_REJECTED;
use super::similarity::{FocusMode, SimilarityInput, hybrid_similarity, segments};
use crate::js::regex::LazyRe;
use crate::js::time::{LocalZone, parse_date};
use crate::js_re;
use crate::stats::schema::{CallFact, CauseId};

pub use super::retry_diff::LearnedDiff;
use super::retry_diff::{command_word_of, culprit_of, diff_of_sides, learned_diff, wait_of};

pub const RETRY_WINDOW: f64 = 3.0;
pub const PAIR_THRESHOLD: f64 = 0.6;
pub const MAX_RETRY_GAP_SEC: f64 = 600.0;

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
    let (Some(fb), Some(rb)) = (&seed.fact.bash, &retry.bash) else {
        return None;
    };
    let arg = seed.fact.cause_arg.as_deref();
    match seed.cause {
        CauseId::CommandNotFound | CauseId::SleepBlocked if arg.is_none() => None,
        CauseId::SleepBlocked => diff_of_sides(arg.unwrap_or(""), &wait_of(retry)),
        CauseId::CommandNotFound => {
            let ran = seed
                .input
                .focus
                .as_ref()
                .and_then(|_| command_word_of(segment))?;
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

/// Substrings each `SHELL_EDITS` pattern needs, checked before the regex runs.
const EDIT_GATES: [&[&str]; 5] = [&["-"], &["open("], &["write"], &["cat", ">"], &["tee"]];

fn shell_line_of(f: &CallFact) -> Option<&str> {
    match &f.bash {
        Some(b) => Some(&b.cmd),
        None if f.tool == "exec_command" => f.arg.as_deref(),
        None => None,
    }
}

fn edits_file(f: &CallFact) -> bool {
    if FILE_EDIT_TOOLS.contains(&f.tool.as_str())
        || f.bash.as_ref().is_some_and(|b| b.bin == "apply_patch")
    {
        return true;
    }
    shell_line_of(f).is_some_and(|l| {
        SHELL_EDITS
            .iter()
            .zip(EDIT_GATES)
            .any(|(re, gate)| gate.iter().all(|g| l.contains(g)) && re.is_match(l))
    })
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
    let same_tool =
        retry.tool == seed.fact.tool || (seed.cause == CauseId::CommandNotFound && retry.is_bash());
    if !same_tool || are_siblings(&seed.fact, retry) {
        return false;
    }
    if clock
        .gap(&seed.fact, retry)
        .is_some_and(|g| !(0.0..=MAX_RETRY_GAP_SEC).contains(&g))
    {
        return false;
    }
    !(seed.cause == CauseId::Unclassified && seed.edit_seen)
}

fn same_call(a: &CallFact, b: &CallFact) -> bool {
    a.base.sid == b.base.sid && a.seq == b.seq
}

fn reads_back(seed: &Seed, f: &CallFact) -> bool {
    let Some(name) = seed.fact.cause_arg.as_deref() else {
        return false;
    };
    if same_call(&seed.fact, f) || are_siblings(&seed.fact, f) {
        return false;
    }
    f.arg.as_deref().is_some_and(|a| a.contains(name))
        || f.bash.as_ref().is_some_and(|b| b.cmd.contains(name))
}

fn close_seed(seed: Seed, clock: Clock<'_>, out: &mut Vec<RetryPair>) {
    let Some(best) = seed.best.as_ref() else {
        return;
    };
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

fn verdict_of(
    seed: &Seed,
    f: &CallFact,
    split: &dyn Fn(&str) -> Vec<String>,
    clock: Clock<'_>,
) -> Verdict {
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
                Some(b) if better.is_none() && score > b.score => {
                    better = Some((i, score, segment));
                }
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
