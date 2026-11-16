//! `scanning 1,234/7,169 transcripts…` on a TTY stderr while a stats pass
//! runs: drawn after a quiet start, redrawn at most every 100 ms, cleared at
//! the end. Clearing overwrites with spaces, not an ANSI erase, so legacy
//! Windows consoles and `TERM=dumb` terminals show no escape garbage.

use std::cell::{Cell, RefCell};

use peektrace_core::stats::format::count;

const DELAY_MS: f64 = 250.0;
const EVERY_MS: f64 = 100.0;

/// Draw only on a terminal that can redraw a line in place.
pub fn wanted(is_tty: bool, term: Option<&str>) -> bool {
    is_tty && term != Some("dumb")
}

/// A progress line over an injected clock and writer.
pub struct Progress<N: Fn() -> f64, W: FnMut(&str)> {
    now: N,
    write: RefCell<W>,
    start: f64,
    last: Cell<f64>,
    /// Width of the line on screen, 0 when nothing is drawn.
    drawn: Cell<usize>,
}

impl<N: Fn() -> f64, W: FnMut(&str)> Progress<N, W> {
    pub fn new(now: N, write: W) -> Self {
        let start = now();
        Self {
            now,
            write: RefCell::new(write),
            start,
            last: Cell::new(f64::NEG_INFINITY),
            drawn: Cell::new(0),
        }
    }

    #[expect(clippy::cast_precision_loss, reason = "file counts fit f64")]
    pub fn on_file(&self, done: usize, total: usize) {
        let t = (self.now)();
        if t - self.start < DELAY_MS || (t - self.last.get() < EVERY_MS && done != total) {
            return;
        }
        self.last.set(t);
        let line = format!(
            "scanning {}/{} transcripts…",
            count(done as f64),
            count(total as f64)
        );
        let width = line.chars().count();
        // Pad over a longer previous line so no stale digits remain.
        let pad = self.drawn.replace(width).saturating_sub(width);
        (self.write.borrow_mut())(&format!("\r{line}{}", " ".repeat(pad)));
    }

    pub fn clear(&self) {
        let width = self.drawn.replace(0);
        if width > 0 {
            (self.write.borrow_mut())(&format!("\r{}\r", " ".repeat(width)));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    fn wipe(line: &str) -> String {
        format!("\r{}\r", " ".repeat(line.chars().count()))
    }

    fn rig() -> (Rc<Cell<f64>>, Rc<RefCell<String>>) {
        (
            Rc::new(Cell::new(0.0)),
            Rc::new(RefCell::new(String::new())),
        )
    }

    #[test]
    fn skipped_off_a_tty_and_on_a_dumb_terminal() {
        assert!(wanted(true, None));
        assert!(wanted(true, Some("xterm-256color")));
        assert!(!wanted(true, Some("dumb")));
        assert!(!wanted(false, Some("xterm")));
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
            format!(
                "\rscanning 1/1,500 transcripts…\rscanning 1,500/1,500 transcripts…{}",
                wipe("scanning 1,500/1,500 transcripts…")
            )
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
            format!(
                "\rscanning 3/3 transcripts…{}",
                wipe("scanning 3/3 transcripts…")
            )
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
        assert!(out.borrow().ends_with(&format!(
            "transcripts…{}",
            wipe("scanning 2,240/2,240 transcripts…")
        )));
    }

    #[test]
    fn clear_writes_no_escape_codes_and_pads_a_shorter_redraw() {
        let (clock, out) = rig();
        let (c, o) = (clock.clone(), out.clone());
        let p = Progress::new(move || c.get(), move |s: &str| o.borrow_mut().push_str(s));
        clock.set(300.0);
        p.on_file(1000, 1000);
        clock.set(500.0);
        p.on_file(9, 9);
        p.clear();
        assert_eq!(
            out.borrow().as_str(),
            format!(
                "\rscanning 1,000/1,000 transcripts…\rscanning 9/9 transcripts…{}{}",
                " ".repeat(8),
                wipe("scanning 9/9 transcripts…")
            )
        );
        assert!(!out.borrow().contains('\x1b'));
    }
}
