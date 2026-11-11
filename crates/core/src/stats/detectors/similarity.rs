//! Python `difflib.SequenceMatcher(None, a, b).ratio()` on code points, plus
//! the segment-aware hybrid score the retry pairer uses.

use std::collections::HashMap;

use crate::js::regex::LazyRe;
use crate::js::text::{self, Unit};
use crate::js_re;

const RATIO_MAX: usize = 800;
const AUTOJUNK_MIN: usize = 200;
const AUTOJUNK_DIVISOR: usize = 100;
const SLEEP_HIT: f64 = 0.9;

/// Code points of `s` as Python indexes it (a lone surrogate is its own unit), at most `max`.
fn points_of(s: &str, max: usize) -> Vec<u32> {
    text::units(s)
        .map(|u| match u {
            Unit::Char(c) => u32::from(c),
            Unit::Lone(n) => u32::from(n),
        })
        .take(max)
        .collect()
}

fn index_b(b: &[u32]) -> HashMap<u32, Vec<usize>> {
    let mut b2j: HashMap<u32, Vec<usize>> = HashMap::new();
    for (j, &ch) in b.iter().enumerate() {
        b2j.entry(ch).or_default().push(j);
    }
    if b.len() >= AUTOJUNK_MIN {
        let ntest = b.len() / AUTOJUNK_DIVISOR + 1;
        b2j.retain(|_, list| list.len() <= ntest);
    }
    b2j
}

struct Matcher<'a> {
    a: &'a [u32],
    b: &'a [u32],
    b2j: HashMap<u32, Vec<usize>>,
    cur: Vec<u32>,
    next: Vec<u32>,
    cur_touched: Vec<usize>,
    next_touched: Vec<usize>,
}

impl Matcher<'_> {
    /// `find_longest_match(alo, ahi, blo, bhi)` with no junk predicate.
    #[expect(
        clippy::similar_names,
        clippy::needless_range_loop,
        reason = "follows difflib"
    )]
    fn longest(&mut self, alo: usize, ahi: usize, blo: usize, bhi: usize) -> (usize, usize, usize) {
        let (a, b) = (self.a, self.b);
        let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0usize);
        for i in alo..ahi {
            if let Some(js) = self.b2j.get(&a[i]) {
                for &j in js {
                    if j < blo {
                        continue;
                    }
                    if j >= bhi {
                        break;
                    }
                    let k = self.cur[j] + 1;
                    self.next[j + 1] = k;
                    self.next_touched.push(j + 1);
                    let k = k as usize;
                    if k > bestsize {
                        besti = i + 1 - k;
                        bestj = j + 1 - k;
                        bestsize = k;
                    }
                }
            }
            for &t in &self.cur_touched {
                self.cur[t] = 0;
            }
            self.cur_touched.clear();
            std::mem::swap(&mut self.cur, &mut self.next);
            std::mem::swap(&mut self.cur_touched, &mut self.next_touched);
        }
        for &t in &self.cur_touched {
            self.cur[t] = 0;
        }
        self.cur_touched.clear();
        while besti > alo && bestj > blo && a[besti - 1] == b[bestj - 1] {
            besti -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while besti + bestsize < ahi
            && bestj + bestsize < bhi
            && a[besti + bestsize] == b[bestj + bestsize]
        {
            bestsize += 1;
        }
        (besti, bestj, bestsize)
    }
}

#[expect(clippy::many_single_char_names, reason = "follows difflib")]
fn matched_points(a: &[u32], b: &[u32]) -> usize {
    let mut m = Matcher {
        a,
        b,
        b2j: index_b(b),
        cur: vec![0; b.len() + 1],
        next: vec![0; b.len() + 1],
        cur_touched: Vec::new(),
        next_touched: Vec::new(),
    };
    let mut queue = vec![(0, a.len(), 0, b.len())];
    let mut total = 0;
    while let Some((alo, ahi, blo, bhi)) = queue.pop() {
        let (i, j, k) = m.longest(alo, ahi, blo, bhi);
        if k == 0 {
            continue;
        }
        total += k;
        if alo < i && blo < j {
            queue.push((alo, i, blo, j));
        }
        if i + k < ahi && j + k < bhi {
            queue.push((i + k, ahi, j + k, bhi));
        }
    }
    total
}

#[expect(clippy::cast_precision_loss, reason = "lengths are at most 800")]
fn ratio_of_points(a: &[u32], b: &[u32]) -> f64 {
    let length = a.len() + b.len();
    if length == 0 {
        return 1.0;
    }
    (2 * matched_points(a, b)) as f64 / length as f64
}

#[expect(clippy::cast_precision_loss, reason = "lengths are at most 800")]
fn quick_ratio_of_points(a: &[u32], b: &[u32]) -> f64 {
    let length = a.len() + b.len();
    if length == 0 {
        return 1.0;
    }
    let mut avail: HashMap<u32, usize> = HashMap::new();
    for &ch in b {
        *avail.entry(ch).or_default() += 1;
    }
    let matches = a
        .iter()
        .filter(|ch| match avail.get_mut(ch) {
            Some(n) if *n > 0 => {
                *n -= 1;
                true
            }
            _ => false,
        })
        .count();
    (2 * matches) as f64 / length as f64
}

/// `SequenceMatcher(None, a[:800], b[:800]).ratio()`.
pub fn ratio(a: &str, b: &str) -> f64 {
    ratio_of_points(&points_of(a, RATIO_MAX), &points_of(b, RATIO_MAX))
}

/// The ratio, or 0 when a cheap upper bound already falls under `floor`.
#[expect(clippy::cast_precision_loss, reason = "lengths are at most 800")]
fn ratio_at_least(a: &str, b: &str, floor: f64) -> f64 {
    let pa = points_of(a, RATIO_MAX);
    let pb = points_of(b, RATIO_MAX);
    let length = pa.len() + pb.len();
    let bound = (2 * pa.len().min(pb.len())) as f64 / length as f64;
    if length > 0 && bound < floor {
        return 0.0;
    }
    if quick_ratio_of_points(&pa, &pb) < floor {
        return 0.0;
    }
    ratio_of_points(&pa, &pb)
}

static QUOTES: LazyRe = js_re!(r#"["']"#, "g");
static ESCAPED_PIPE: LazyRe = js_re!(r"\\\|", "g");
static SEGMENT_SPLIT: LazyRe = js_re!(r"\s*(?:&&|\|\||;|\||\n)\s*");

/// Quotes dropped, `\|` unescaped, whitespace collapsed, trimmed.
pub fn norm_seg(s: &str) -> String {
    let s = QUOTES.replace_all_str(s, "");
    let s = ESCAPED_PIPE.replace_all_str(&s, "|");
    super::super::rank::collapse_ws(&s)
}

const NOISE_PREFIXES: [&str; 7] = ["echo", "cd ", "head", "tail -", "wc", "sort", "uniq"];

/// The intent-carrying segments of a command, normalised.
pub fn segments(cmd: &str) -> Vec<String> {
    let kept: Vec<String> = SEGMENT_SPLIT
        .split(cmd)
        .into_iter()
        .filter(|x| {
            let t = text::trim(x);
            !t.is_empty() && !NOISE_PREFIXES.iter().any(|p| t.starts_with(p))
        })
        .map(norm_seg)
        .collect();
    if kept.is_empty() {
        vec![norm_seg(cmd)]
    } else {
        kept
    }
}

/// Separator between the segments stored in a fact's `focus`.
pub const FOCUS_SEP: &str = " ␞ ";

/// How a failed call's focus was cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusMode {
    Glob,
    Cnf,
    Sleep,
    Whole,
}

#[derive(Debug, Clone)]
pub struct SimilarityInput {
    pub focus: Option<String>,
    pub mode: FocusMode,
    pub shell: bool,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SimilarityHit {
    pub score: f64,
    pub segment: String,
}

fn drop_first_word(s: &str) -> String {
    s.split(' ').skip(1).collect::<Vec<_>>().join(" ")
}

fn last_word(s: &str) -> &str {
    s.split(' ').next_back().unwrap_or("")
}

/// Score a retry against a failed call: whole lines, or the failing
/// segments against each retry segment.
pub fn hybrid_similarity(
    failed: &SimilarityInput,
    retry: &str,
    floor: f64,
    split: &dyn Fn(&str) -> Vec<String>,
) -> SimilarityHit {
    let score = |a: &str, b: &str, bar: f64| {
        if bar > 0.0 {
            ratio_at_least(a, b, bar)
        } else {
            ratio(a, b)
        }
    };
    let focus = match (&failed.focus, failed.shell, failed.mode) {
        (Some(f), true, mode) if mode != FocusMode::Whole => f,
        _ => {
            return SimilarityHit {
                score: score(&failed.text, retry, floor),
                segment: retry.to_owned(),
            };
        }
    };
    let base = split(retry);
    let mut candidates: Vec<(String, &str)> =
        base.iter().map(|s| (s.clone(), s.as_str())).collect();
    if failed.mode == FocusMode::Cnf {
        candidates.extend(base.iter().map(|s| (drop_first_word(s), s.as_str())));
    }
    let mut best = SimilarityHit {
        score: 0.0,
        segment: base.first().cloned().unwrap_or_default(),
    };
    for f in focus.split(FOCUS_SEP).filter(|s| !s.is_empty()) {
        for (cmp, seg) in &candidates {
            let bar = if floor > 0.0 {
                floor.max(best.score)
            } else {
                0.0
            };
            let mut s = score(f, cmp, bar);
            if failed.mode == FocusMode::Sleep && cmp.contains(last_word(f)) {
                s = s.max(SLEEP_HIT);
            }
            if s > best.score {
                best = SimilarityHit {
                    score: s,
                    segment: (*seg).to_owned(),
                };
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn difflib_golden() {
        let raw =
            include_str!("../../../../../packages/core/test/fixtures/stats/difflib-golden.json");
        let v: serde_json::Value = serde_json::from_str(raw).unwrap_or_default();
        let cases = v.as_array().cloned().unwrap_or_default();
        assert!(cases.len() >= 20);
        for c in cases {
            let a = c["a"].as_str().unwrap_or_default();
            let b = c["b"].as_str().unwrap_or_default();
            let r = c["r"].as_f64().unwrap_or(-1.0);
            assert!(
                ratio(a, b).to_bits() == r.to_bits(),
                "{a:?} {b:?} {} != {r}",
                ratio(a, b)
            );
        }
    }

    #[test]
    fn segments_drop_noise() {
        assert_eq!(segments("cd x && ls -la | head -5"), vec!["ls -la"]);
        assert_eq!(norm_seg("  grep 'a\\|b'  f "), "grep a|b f");
    }
}
