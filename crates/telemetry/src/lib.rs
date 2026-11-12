//! Local wide-event telemetry: one JSON event per root span, kept in a
//! SQLite file that the TS CLI shares (`packages/telemetry`). The layer
//! builds events in memory; a background thread writes them. Nothing here
//! prints, and a broken store never fails a command.

pub mod attrs;
pub mod event;
pub mod layer;
pub mod store;
pub mod writer;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub use attrs::Attrs;
pub use event::{Kind, Outcome, SpanRecord, WideEvent};
pub use layer::{SpansOnly, StoreLayer};
pub use store::{Row, Store, StoreError};
pub use writer::Writer;

/// How long a process waits at exit for its events to land.
pub const EXIT_WAIT: Duration = Duration::from_millis(300);

static WRITER: OnceLock<Writer> = OnceLock::new();

/// Start the process writer for `dir` (`None`: record nothing) and return
/// the layer that feeds it.
pub fn start(dir: Option<PathBuf>, app_version: &str, now_ms: i64) -> StoreLayer {
    let writer = dir.map_or_else(Writer::disabled, |d| Writer::start(d, now_ms));
    let feed = writer.clone();
    let _ = WRITER.set(writer);
    install_panic_hook();
    StoreLayer::new(
        Arc::new(move |e: WideEvent| feed.send(Row::of(&e))),
        app_version,
        event::platform(),
    )
}

/// Wait up to `limit` for queued events to be written. No-op when
/// telemetry never started.
pub fn flush(limit: Duration) {
    if let Some(w) = WRITER.get() {
        w.flush(limit);
    }
}

/// Wait up to `limit` for the process writer to open its store. No-op when
/// telemetry never started.
pub fn wait_ready(limit: Duration) {
    if let Some(w) = WRITER.get() {
        w.wait_ready(limit);
    }
}

/// A panic marks the innermost entered root as a defect, then runs the
/// hook that was there before.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        layer::mark_defect(&layer::panic_message(info.payload()));
        previous(info);
    }));
}
