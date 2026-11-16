//! `peektrace tui`: the terminal UI over core, in process, with the web
//! server serving the same roots on a thread beside it.

use std::io::IsTerminal as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use peektrace_core::header_cache::HeaderCache;
use peektrace_tui::backend::CoreBackend;
use peektrace_tui::theme::ColorMode;
use peektrace_tui::{Config, restore_terminal, run as run_ui};

use crate::ls::{Context, UserError};
use crate::otel;
use crate::serve::{self, Prepared};
use crate::zone::SystemZone;

/// The TUI draws only on a terminal.
pub fn check_terminal() -> Result<(), UserError> {
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        Ok(())
    } else {
        Err(UserError::Invalid(
            "peektrace tui needs a terminal".to_owned(),
        ))
    }
}

/// How long the draw loop gets to notice a stop signal.
const STUCK: Duration = Duration::from_secs(1);

/// A closed terminal can leave the draw loop inside a read that sees end
/// of file over and over, so it never gets back to check the stop flag.
/// After a signal, give it `STUCK`, then restore and exit for it.
fn watchdog(signalled: Receiver<()>, finished: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        if signalled.recv().is_err() {
            return;
        }
        std::thread::sleep(STUCK);
        if !finished.load(Ordering::Relaxed) {
            restore_terminal();
            otel::release();
            crate::signals::exit_recorded(0);
        }
    });
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
        .floor()
}

pub fn run(p: Prepared, _ctx: &Context) -> anyhow::Result<()> {
    let url = p.banner.open_url.clone();
    let authed = url.contains("token=");
    let open = p.open;
    let sibling = serve::spawn(p)?;
    for w in &sibling.warnings {
        eprintln!("{w}");
    }
    if open {
        serve::open(&url);
    }
    let lock = Arc::clone(&sibling.stats_lock);
    let backend = CoreBackend {
        roots: sibling.roots.clone(),
        settings_path: sibling.settings_path.clone(),
        read_only: sibling.read_only,
        zone: Arc::new(SystemZone::default()),
        headers: HeaderCache::default(),
        stats_cache_root: sibling.stats_cache_root.clone(),
        stats_gate: Arc::new(move |f: &mut dyn FnMut()| {
            let _held = lock.blocking_lock();
            f();
        }),
        now: Arc::new(now_ms),
    };
    let cfg = Config {
        url,
        authed,
        color: ColorMode::detect(|k| std::env::var(k).ok()),
        now: Arc::new(now_ms),
    };
    otel::hold();
    let (versions, stopped) = (sibling.versions, Arc::clone(&sibling.stopped));
    let finished = Arc::new(AtomicBool::new(false));
    watchdog(sibling.signalled, Arc::clone(&finished));
    let result = run_ui(cfg, Arc::new(backend), &versions, &stopped);
    finished.store(true, Ordering::Relaxed);
    otel::release();
    sibling.handle.stop();
    result.map_err(Into::into)
}
