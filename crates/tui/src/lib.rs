//! `peektrace tui`: the terminal UI over `peektrace-core`, in process.

mod app;
pub mod backend;
mod draw;
pub mod fmt;
mod input;
pub mod list;
pub mod runtime;
pub mod screens;
mod syntax;
mod terminal;
pub mod theme;
mod widgets;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

pub use app::{App, Config, Section};
use peektrace_core::watch::WatchVersions;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::layout::Rect;
pub use terminal::restore as restore_terminal;

use crate::backend::Backend;
use crate::runtime::Dispatcher;

const POLL: Duration = Duration::from_millis(40);
const TICK: Duration = Duration::from_secs(1);

/// Draw the TUI until the user quits, `stop` turns true, or `watch` and
/// the terminal both go away. The terminal is restored before return.
pub fn run(
    cfg: Config,
    backend: Arc<dyn Backend>,
    watch: &Receiver<WatchVersions>,
    stop: &AtomicBool,
) -> std::io::Result<()> {
    let mut term = terminal::enter()?;
    let result = run_loop(&mut term, cfg, backend, watch, stop);
    terminal::restore();
    result
}

fn run_loop(
    term: &mut terminal::Term,
    cfg: Config,
    backend: Arc<dyn Backend>,
    watch: &Receiver<WatchVersions>,
    stop: &AtomicBool,
) -> std::io::Result<()> {
    let size = term.size()?;
    let mut app = App::new(cfg, Rect::new(0, 0, size.width, size.height));
    let (tx, replies) = mpsc::channel();
    let mut work = Dispatcher::new(backend, tx);
    let mut dirty = true;
    let mut last_tick = Instant::now();
    while !app.quit() && !stop.load(Ordering::Relaxed) {
        for job in app.take_jobs() {
            work.submit(job);
        }
        if dirty {
            term.draw(|f| app.draw(f.buffer_mut()))?;
            dirty = false;
        }
        while let Ok(r) = replies.try_recv() {
            work.done(&r);
            app.reply(r);
            dirty = true;
        }
        while let Ok(v) = watch.try_recv() {
            app.watch(v);
            dirty = true;
        }
        if dirty {
            continue;
        }
        if event::poll(POLL)? {
            match event::read()? {
                Event::Key(k) if k.kind != KeyEventKind::Release => app.key(k),
                Event::Mouse(m) => app.mouse(m),
                Event::Paste(s) => app.paste(&s),
                Event::Resize(w, h) => app.resize(Rect::new(0, 0, w, h)),
                _ => continue,
            }
            dirty = true;
        } else {
            app.tick();
            if last_tick.elapsed() >= TICK {
                last_tick = Instant::now();
                dirty = app.ticking();
            }
        }
    }
    Ok(())
}
