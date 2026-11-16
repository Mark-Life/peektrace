//! The five sections. Each keeps its state for the whole run.

pub mod capabilities;
pub mod memory;
pub mod sessions;
pub mod settings;
pub mod stats;

use peektrace_core::watch::WatchVersions;
use ratatui::layout::Rect;

use crate::backend::{Job, Request};

/// What a screen may read and ask for while it handles an event.
pub struct Ctx<'a> {
    jobs: &'a mut Vec<Job>,
    next_id: &'a mut u64,
    pub watch: WatchVersions,
    pub now: f64,
    /// The screen's body area, as last drawn.
    pub area: Rect,
}

impl<'a> Ctx<'a> {
    pub fn new(
        jobs: &'a mut Vec<Job>,
        next_id: &'a mut u64,
        watch: WatchVersions,
        now: f64,
        area: Rect,
    ) -> Self {
        Self {
            jobs,
            next_id,
            watch,
            now,
            area,
        }
    }

    /// Queue a backend call; the reply carries the returned id.
    pub fn request(&mut self, req: Request) -> u64 {
        *self.next_id += 1;
        let id = *self.next_id;
        self.jobs.push(Job { id, req });
        id
    }
}

/// One query's state: the last good data, the last error, and the request
/// whose reply it waits for. A reply for any other id is stale.
#[derive(Debug, Clone)]
pub struct Query<T> {
    pub data: Option<T>,
    pub error: Option<String>,
    req: Option<u64>,
}

impl<T> Default for Query<T> {
    fn default() -> Self {
        Self {
            data: None,
            error: None,
            req: None,
        }
    }
}

impl<T> Query<T> {
    pub fn start(&mut self, id: u64) {
        self.req = Some(id);
        self.error = None;
    }

    pub const fn loading(&self) -> bool {
        self.req.is_some()
    }

    pub fn waits_for(&self, id: u64) -> bool {
        self.req == Some(id)
    }

    /// Take the reply when it is the one awaited; errors keep the old data.
    pub fn land(&mut self, id: u64, r: Result<T, String>) -> bool {
        if self.req != Some(id) {
            return false;
        }
        self.req = None;
        match r {
            Ok(d) => {
                self.data = Some(d);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        true
    }
}

/// A mouse target, stored while drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Card(usize),
    Rail,
    HistoryRow(usize),
    History,
    ExpandAll,
    CollapseAll,
    BiggestFirst,
    ViewHistory,
    ViewGrowth,
}
