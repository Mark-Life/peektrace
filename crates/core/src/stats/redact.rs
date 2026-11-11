//! The one route every derived string takes before it leaves core.

use std::cell::RefCell;
use std::collections::HashMap;

use super::rank::{SAMPLE_LINE_MAX, clip, collapse_ws};
use crate::sessions::redact::redact_text;

const MEMO_CAP: usize = 4096;

/// Redacts derived strings, memoising repeats (command text repeats heavily).
#[derive(Debug, Default)]
pub struct Redactor {
    pub enabled: bool,
    memo: RefCell<HashMap<String, String>>,
}

impl Redactor {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            memo: RefCell::default(),
        }
    }

    /// Redact one derived string.
    pub fn text(&self, s: &str) -> String {
        if !self.enabled || s.is_empty() {
            return s.to_owned();
        }
        if let Some(hit) = self.memo.borrow().get(s) {
            return hit.clone();
        }
        let out = redact_text(s).into_owned();
        let mut memo = self.memo.borrow_mut();
        if memo.len() >= MEMO_CAP {
            memo.clear();
        }
        memo.insert(s.to_owned(), out.clone());
        out
    }

    /// Redact, collapse whitespace and clip to `max` units.
    pub fn line_max(&self, s: &str, max: usize) -> String {
        clip(&collapse_ws(&self.text(s)), max)
    }

    pub fn line(&self, s: &str) -> String {
        self.line_max(s, SAMPLE_LINE_MAX)
    }

    /// `None` and `""` become `None`, anything else is redacted.
    pub fn nullable(&self, s: Option<&str>) -> Option<String> {
        s.filter(|s| !s.is_empty()).map(|s| self.text(s))
    }
}
