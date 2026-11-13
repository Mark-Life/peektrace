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

/// Emit every root span still open, as if it closed now. Call before an
/// exit that skips unwinding, then `flush`. No-op without the store layer.
pub fn finish_open_roots() {
    tracing::dispatcher::get_default(|d| {
        if let (Some(layer), Some(spans)) = (
            d.downcast_ref::<StoreLayer>(),
            d.downcast_ref::<tracing_subscriber::Registry>(),
        ) {
            layer.finish_open(spans);
        }
    });
}

/// Mark the innermost root entered on this thread as a defect, from a
/// caught panic payload. Covers panics raised on another thread and
/// re-raised here (rayon), which skip the hook on this thread.
pub fn record_panic(payload: &(dyn std::any::Any + Send)) {
    layer::mark_defect(&layer::panic_message(payload));
}

/// Run `f` inside a root; a panic marks that root as a defect, then keeps
/// unwinding. Needed where `f` fans out to other threads (rayon): their
/// panics come back without running the hook on this thread.
pub fn defect_on_panic<T>(f: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|payload| {
        record_panic(payload.as_ref());
        std::panic::resume_unwind(payload)
    })
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
