//! `scanning 1,234/7,169 transcripts…` on a TTY stderr while a stats pass
//! runs: drawn after a quiet start, redrawn at most every 100 ms, cleared at
//! the end.

use std::cell::{Cell, RefCell};

use peektrace_core::stats::format::count;

const DELAY_MS: f64 = 250.0;
const EVERY_MS: f64 = 100.0;
const CLEAR: &str = "\r\x1b[2K";

/// A progress line over an injected clock and writer.
pub struct Progress<N: Fn() -> f64, W: FnMut(&str)> {
    now: N,
    write: RefCell<W>,
    start: f64,
    last: Cell<f64>,
    drawn: Cell<bool>,
}

impl<N: Fn() -> f64, W: FnMut(&str)> Progress<N, W> {
    pub fn new(now: N, write: W) -> Self {
        let start = now();
        Self {
            now,
            write: RefCell::new(write),
            start,
            last: Cell::new(f64::NEG_INFINITY),
            drawn: Cell::new(false),
        }
    }

    #[expect(clippy::cast_precision_loss, reason = "file counts fit f64")]
    pub fn on_file(&self, done: usize, total: usize) {
        let t = (self.now)();
        if t - self.start < DELAY_MS || (t - self.last.get() < EVERY_MS && done != total) {
            return;
        }
        self.last.set(t);
        self.drawn.set(true);
        (self.write.borrow_mut())(&format!(
            "\rscanning {}/{} transcripts…",
            count(done as f64),
            count(total as f64)
        ));
    }

    pub fn clear(&self) {
        if self.drawn.replace(false) {
            (self.write.borrow_mut())(CLEAR);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    fn rig() -> (Rc<Cell<f64>>, Rc<RefCell<String>>) {
        (
            Rc::new(Cell::new(0.0)),
            Rc::new(RefCell::new(String::new())),
        )
    }

    #[test]
    fn quiet_start_draws_nothing() {
        let (clock, out) = rig();
        let (c, o) = (clock.clone(), out.clone());
        let p = Progress::new(move || c.get(), move |s: &str| o.borrow_mut().push_str(s));
        clock.set(100.0);
        p.on_file(1, 10);
        p.clear();
        assert_eq!(out.borrow().as_str(), "");
    }

    #[test]
    fn throttles_but_draws_the_last_file() {
        let (clock, out) = rig();
        let (c, o) = (clock.clone(), out.clone());
        let p = Progress::new(move || c.get(), move |s: &str| o.borrow_mut().push_str(s));
        clock.set(300.0);
        p.on_file(1, 1500);
        clock.set(350.0);
        p.on_file(2, 1500);
        p.on_file(1500, 1500);
        p.clear();
        assert_eq!(
            out.borrow().as_str(),
            "\rscanning 1/1,500 transcripts…\rscanning 1,500/1,500 transcripts…\r\x1b[2K"
        );
    }

    #[test]
    fn clear_only_after_a_draw() {
        let (clock, out) = rig();
        let (c, o) = (clock.clone(), out.clone());
        let p = Progress::new(move || c.get(), move |s: &str| o.borrow_mut().push_str(s));
        p.clear();
        clock.set(400.0);
        p.on_file(3, 3);
        p.clear();
        p.clear();
        assert_eq!(
            out.borrow().as_str(),
            "\rscanning 3/3 transcripts…\r\x1b[2K"
        );
    }

    #[test]
    fn a_pass_inside_the_delay_prints_nothing() {
        let (clock, out) = rig();
        let (c, o) = (clock.clone(), out.clone());
        let p = Progress::new(move || c.get(), move |s: &str| o.borrow_mut().push_str(s));
        clock.set(100.0);
        p.on_file(1, 2);
        clock.set(200.0);
        p.on_file(2, 2);
        p.clear();
        assert_eq!(out.borrow().as_str(), "");
    }

    #[test]
    fn redraws_at_most_every_100ms_plus_the_final_count() {
        let (clock, out) = rig();
        let (c, o) = (clock.clone(), out.clone());
        let p = Progress::new(move || c.get(), move |s: &str| o.borrow_mut().push_str(s));
        for (at, done) in [(300.0, 1), (350.0, 2), (400.0, 1240), (410.0, 2240)] {
            clock.set(at);
            p.on_file(done, 2240);
        }
        assert_eq!(
            out.borrow().as_str(),
            "\rscanning 1/2,240 transcripts…\rscanning 1,240/2,240 transcripts…\rscanning 2,240/2,240 transcripts…"
        );
        p.clear();
        p.clear();
        assert!(out.borrow().ends_with("transcripts…\r\x1b[2K"));
    }
}
