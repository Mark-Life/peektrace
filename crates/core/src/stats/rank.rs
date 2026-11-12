//! Comparators, rounding and record helpers every ranked surface uses, so two
//! runs over an unchanged corpus print the same bytes.

use std::cmp::Ordering;
use std::collections::HashSet;

use indexmap::IndexMap;

use crate::js::num::js_round;
use crate::js::{text, units};

pub const DEFAULT_ROW_LIMIT: f64 = 20.0;
pub const DEFAULT_SAMPLE_CAP: usize = 3;
pub const DEFAULT_DRILL_PAGE: usize = 200;
pub const SAMPLE_LINE_MAX: usize = 160;
pub const SAMPLE_POOL_LINE_MAX: usize = 400;
pub const SAMPLE_POOL: usize = 64;

/// JS `a < b` string order (UTF-16 code units).
pub fn cmp_str(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        return a.cmp(b);
    }
    text::cmp_utf16(a, b)
}

/// `b - a` on numbers as an ordering, NaN as equal.
pub fn desc(a: f64, b: f64) -> Ordering {
    b.partial_cmp(&a).unwrap_or(Ordering::Equal)
}

pub fn asc(a: f64, b: f64) -> Ordering {
    a.partial_cmp(&b).unwrap_or(Ordering::Equal)
}

/// Round to `digits` places; non-finite becomes 0 and `-0` becomes `0`.
pub fn round_to(n: f64, digits: i32) -> f64 {
    if !n.is_finite() {
        return 0.0;
    }
    let p = 10f64.powi(digits);
    let v = js_round(n * p) / p;
    if v == 0.0 { 0.0 } else { v }
}

pub fn round(n: f64) -> f64 {
    round_to(n, 2)
}

pub fn safe_div(a: f64, b: f64) -> f64 {
    if b == 0.0 || !b.is_finite() {
        0.0
    } else {
        a / b
    }
}

pub fn share(part: f64, total: f64) -> f64 {
    round_to(safe_div(part, total), 4)
}

fn sorted(values: &[f64]) -> Vec<f64> {
    let mut v = values.to_vec();
    v.sort_by(|a, b| asc(*a, *b));
    v
}

/// Nearest-rank percentile; 0 for an empty list.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let s = sorted(values);
    let n = s.len();
    let idx = ((p * n as f64).ceil() - 1.0).max(0.0) as usize;
    s[idx.min(n - 1)]
}

pub fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let s = sorted(values);
    let mid = s.len() / 2;
    if s.len() % 2 == 1 {
        s[mid]
    } else {
        f64::midpoint(s[mid - 1], s[mid])
    }
}

/// Sorted unique strings.
pub fn uniq_sorted<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut v: Vec<&str> = values
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    v.sort_by(|a, b| cmp_str(a, b));
    v.into_iter().map(str::to_owned).collect()
}

/// Key with the highest count; ties break on the smallest key.
pub fn top_key(counts: &IndexMap<String, f64>) -> String {
    let mut best: Option<(&str, f64)> = None;
    for (k, &n) in counts {
        let better = match best {
            None => true,
            Some((bk, bn)) => n > bn || (n == bn && cmp_str(k, bk) == Ordering::Less),
        };
        if better {
            best = Some((k, n));
        }
    }
    best.map_or(String::new(), |(k, _)| k.to_owned())
}

pub fn bump(counts: &mut IndexMap<String, f64>, key: &str, by: f64) {
    *counts.entry(key.to_owned()).or_insert(0.0) += by;
}

/// A capped ranking that reports how many rows it dropped.
#[derive(Debug, Clone)]
pub struct Ranking<T> {
    pub rows: Vec<T>,
    pub total: usize,
    pub dropped: usize,
}

/// `max(0, floor(limit))` as a row count.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn cap_of(limit: f64) -> usize {
    if limit.is_nan() {
        0
    } else {
        limit.floor().clamp(0.0, 1e15) as usize
    }
}

/// Sort by `cmp` (stable), keep the first `limit` rows.
pub fn rank<T>(mut rows: Vec<T>, cmp: impl FnMut(&T, &T) -> Ordering, limit: f64) -> Ranking<T> {
    rows.sort_by(cmp);
    let total = rows.len();
    rows.truncate(cap_of(limit));
    Ranking {
        dropped: total - rows.len(),
        rows,
        total,
    }
}

/// Collapse whitespace runs to one space and trim.
pub fn collapse_ws(s: &str) -> String {
    text::trim(&units::collapse_ws(s)).to_owned()
}

pub fn clip(s: &str, max: usize) -> String {
    units::clip(s, max)
}

/// What a sample pool orders by.
pub trait SampleKey {
    fn line(&self) -> &str;
    fn sid(&self) -> &str;
    fn seq(&self) -> f64;
}

/// Shortest line first, then the line, sid and seq.
pub fn compare_samples<T: SampleKey>(a: &T, b: &T) -> Ordering {
    units::len(a.line())
        .cmp(&units::len(b.line()))
        .then_with(|| compare_rest(a, b))
}

fn compare_rest<T: SampleKey>(a: &T, b: &T) -> Ordering {
    cmp_str(a.line(), b.line())
        .then_with(|| cmp_str(a.sid(), b.sid()))
        .then_with(|| asc(a.seq(), b.seq()))
}

/// Indices of up to `cap` candidates in `compare_samples` order, the first
/// of each call only. Line lengths are counted once per candidate.
fn pick_order<T: SampleKey>(candidates: &[T], cap: usize) -> Vec<usize> {
    let mut seen = HashSet::with_capacity(candidates.len());
    let mut unique: Vec<(usize, usize)> = candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| seen.insert((c.sid(), c.seq().to_bits())))
        .map(|(i, c)| (units::len(c.line()), i))
        .collect();
    let cmp = |a: &(usize, usize), b: &(usize, usize)| {
        a.0.cmp(&b.0)
            .then_with(|| compare_rest(&candidates[a.1], &candidates[b.1]))
            .then_with(|| a.1.cmp(&b.1))
    };
    if unique.len() > cap && cap > 0 {
        unique.select_nth_unstable_by(cap - 1, cmp);
    }
    unique.truncate(cap);
    unique.sort_unstable_by(cmp);
    unique.into_iter().map(|(_, i)| i).collect()
}

/// Up to `cap` samples in `compare_samples` order, one per call.
pub fn pick_samples<T: SampleKey + Clone>(candidates: &[T], cap: usize) -> Vec<T> {
    pick_order(candidates, cap)
        .into_iter()
        .map(|i| candidates[i].clone())
        .collect()
}

/// Cut a pool that grew past twice `SAMPLE_POOL` back to the best `SAMPLE_POOL`.
pub fn bound_pool<T: SampleKey>(pool: &mut Vec<T>) {
    if pool.len() <= SAMPLE_POOL * 2 {
        return;
    }
    let order = pick_order(pool, SAMPLE_POOL);
    let mut slots: Vec<Option<T>> = pool.drain(..).map(Some).collect();
    pool.extend(order.into_iter().filter_map(|i| slots[i].take()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding() {
        assert!((round(1.005) - 1.0).abs() < 1e-12 || (round(1.005) - 1.01).abs() < 1e-12);
        assert_eq!(round(-0.001).to_bits(), 0f64.to_bits());
        assert!((share(1.0, 3.0) - 0.3333).abs() < 1e-12);
        assert_eq!(round(f64::NAN).to_bits(), 0f64.to_bits());
    }

    #[derive(Clone, Debug, PartialEq)]
    struct S(&'static str, &'static str, f64);

    impl SampleKey for S {
        fn line(&self) -> &str {
            self.0
        }
        fn sid(&self) -> &str {
            self.1
        }
        fn seq(&self) -> f64 {
            self.2
        }
    }

    #[test]
    fn picks_match_a_full_sort_of_first_occurrences() {
        let lines = ["bb", "a", "é", "ccc", "a", "\u{1F600}", "zz", "b"];
        let pool: Vec<S> = (0..300)
            .map(|i| {
                S(
                    lines[(i * 7) % lines.len()],
                    ["s1", "s2", "s3"][i % 3],
                    f64::from(u32::try_from(i % 97).unwrap_or(0)),
                )
            })
            .collect();
        let mut seen = HashSet::new();
        let mut want: Vec<S> = pool
            .iter()
            .filter(|c| seen.insert((c.1, c.2.to_bits())))
            .cloned()
            .collect();
        want.sort_by(compare_samples);
        want.truncate(SAMPLE_POOL);
        assert_eq!(pick_samples(&pool, SAMPLE_POOL), want);
        let mut bounded = pool;
        bound_pool(&mut bounded);
        assert_eq!(bounded, want);
        assert_eq!(pick_samples(&bounded, 0), Vec::<S>::new());
    }

    #[test]
    fn percentiles() {
        assert!((percentile(&[3.0, 1.0, 2.0], 0.95) - 3.0).abs() < 1e-12);
        assert!((median(&[4.0, 1.0, 2.0, 3.0]) - 2.5).abs() < 1e-12);
        assert_eq!(percentile(&[], 0.5).to_bits(), 0f64.to_bits());
    }
}
