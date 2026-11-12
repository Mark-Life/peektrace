//! The regex pass every language shares: each pattern claims the first
//! unclaimed spans it matches, in pattern order.

use super::{Range, TokenClass};
use crate::js::regex::{Caps, LazyRe};

/// How a pattern names its matches.
pub(super) enum Class {
    Fixed(TokenClass),
    By(fn(&Caps<'_>) -> TokenClass),
}

pub(super) struct Pattern {
    pub re: &'static LazyRe,
    /// Capture group that is the token; 0 for the whole match.
    pub group: usize,
    pub class: Class,
}

pub(super) const fn pat(re: &'static LazyRe, class: TokenClass) -> Pattern {
    Pattern {
        re,
        group: 0,
        class: Class::Fixed(class),
    }
}

pub(super) const fn group(re: &'static LazyRe, group: usize, class: TokenClass) -> Pattern {
    Pattern {
        re,
        group,
        class: Class::Fixed(class),
    }
}

/// Ranges already claimed, one flag per byte.
pub(super) struct Occupied(Vec<bool>);

impl Occupied {
    pub fn new(len: usize) -> Self {
        Self(vec![false; len])
    }

    pub fn fill(&mut self, r: &Range) {
        if let Some(cells) = self.0.get_mut(r.start..r.end) {
            cells.fill(true);
        }
    }

    /// Push `r` when it is in bounds and free.
    pub fn add(&mut self, ranges: &mut Vec<Range>, r: Range) -> bool {
        if r.start >= r.end || r.end > self.0.len() || self.0[r.start..r.end].iter().any(|b| *b) {
            return false;
        }
        self.fill(&r);
        ranges.push(r);
        true
    }
}

/// `initial` plus every pattern match that does not overlap a claimed span.
pub(super) fn collect(code: &str, patterns: &[Pattern], initial: Vec<Range>) -> Vec<Range> {
    let mut occupied = Occupied::new(code.len());
    for r in &initial {
        occupied.fill(r);
    }
    matches(code, patterns, initial, &mut occupied)
}

/// Like [`collect`], but `first` ranges are claimed one by one, each only
/// when free: a `collect` pattern placed first in the list.
pub(super) fn collect_after(code: &str, first: Vec<Range>, patterns: &[Pattern]) -> Vec<Range> {
    let mut occupied = Occupied::new(code.len());
    let mut ranges = Vec::with_capacity(first.len());
    for r in first {
        occupied.add(&mut ranges, r);
    }
    matches(code, patterns, ranges, &mut occupied)
}

fn matches(
    code: &str,
    patterns: &[Pattern],
    mut ranges: Vec<Range>,
    occupied: &mut Occupied,
) -> Vec<Range> {
    for p in patterns {
        for caps in p.re.captures_all(code) {
            let Some(span) = caps.range(p.group).filter(|s| !s.is_empty()) else {
                continue;
            };
            let class = match p.class {
                Class::Fixed(c) => c,
                Class::By(f) => f(&caps),
            };
            let start = caps.end() - span.len();
            occupied.add(&mut ranges, Range::new(start, start + span.len(), class));
        }
    }
    ranges
}

pub(super) fn offset(ranges: Vec<Range>, by: usize) -> impl Iterator<Item = Range> {
    ranges
        .into_iter()
        .map(move |r| Range::new(r.start + by, r.end + by, r.class))
}
