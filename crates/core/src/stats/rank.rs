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
    if b == 0.0 || !b.is_finite() { 0.0 } else { a / b }
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
#[expect(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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
    let mut v: Vec<&str> = values.into_iter().collect::<HashSet<_>>().into_iter().collect();
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
    if limit.is_nan() { 0 } else { limit.floor().max(0.0).min(1e15) as usize }
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
        .then_with(|| cmp_str(a.line(), b.line()))
        .then_with(|| cmp_str(a.sid(), b.sid()))
        .then_with(|| asc(a.seq(), b.seq()))
}

/// Up to `cap` samples in `compare_samples` order, one per call.
pub fn pick_samples<T: SampleKey + Clone>(candidates: &[T], cap: usize) -> Vec<T> {
    let mut seen = HashSet::new();
    let mut unique: Vec<T> = candidates
        .iter()
        .filter(|c| seen.insert((c.sid().to_owned(), c.seq().to_bits())))
        .cloned()
        .collect();
    unique.sort_by(compare_samples);
    unique.truncate(cap);
    unique
}

/// Cut a pool that grew past twice `SAMPLE_POOL` back to the best `SAMPLE_POOL`.
pub fn bound_pool<T: SampleKey + Clone>(pool: &mut Vec<T>) {
    if pool.len() > SAMPLE_POOL * 2 {
        *pool = pick_samples(pool, SAMPLE_POOL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding() {
        assert!((round(1.005) - 1.0).abs() < 1e-12 || (round(1.005) - 1.01).abs() < 1e-12);
        assert_eq!(round(-0.001).to_bits(), 0f64.to_bits());
        assert!((share(1.0, 3.0) - 0.3333).abs() < 1e-12);
        assert!(round(f64::NAN) == 0.0);
    }

    #[test]
    fn percentiles() {
        assert!((percentile(&[3.0, 1.0, 2.0], 0.95) - 3.0).abs() < 1e-12);
        assert!((median(&[4.0, 1.0, 2.0, 3.0]) - 2.5).abs() < 1e-12);
        assert!(percentile(&[], 0.5) == 0.0);
    }
}
