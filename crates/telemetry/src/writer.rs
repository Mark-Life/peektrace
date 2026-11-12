//! The background writer: events queue to one thread that owns the store
//! connection and writes them in batches. A command never waits on SQLite
//! except for the bounded flush at exit, and a store that cannot open or
//! write drops the events with no output.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::store::{Row, Store};

/// Events queued at most; more are dropped.
pub const QUEUE: usize = 1024;
/// Rows written per transaction at most.
const BATCH: usize = 256;

/// Counts that let a caller wait until what it queued is written.
#[derive(Default)]
struct Progress {
    sent: AtomicU64,
    done: Mutex<u64>,
    changed: Condvar,
    dead: AtomicBool,
}

impl Progress {
    fn finish(&self, n: u64) {
        *self.done.lock().unwrap_or_else(PoisonError::into_inner) += n;
        self.changed.notify_all();
    }

    fn die(&self) {
        self.dead.store(true, Ordering::Relaxed);
        let _held = self.done.lock().unwrap_or_else(PoisonError::into_inner);
        self.changed.notify_all();
    }
}

/// The sending side; cheap to clone.
#[derive(Clone)]
pub struct Writer {
    tx: SyncSender<Row>,
    progress: Arc<Progress>,
}

impl Writer {
    /// Start the writer thread. It opens the store in `dir` at once, so the
    /// open overlaps the command. `now_ms` dates the retention purge.
    pub fn start(dir: PathBuf, now_ms: i64) -> Self {
        let (tx, rx) = sync_channel(QUEUE);
        let progress = Arc::new(Progress::default());
        let shared = Arc::clone(&progress);
        let spawned = std::thread::Builder::new()
            .name("telemetry".to_owned())
            .spawn(move || match Store::open(&dir, now_ms) {
                Ok(store) => drain(store, &rx, &shared),
                Err(_) => shared.die(),
            });
        if spawned.is_err() {
            progress.die();
        }
        Self { tx, progress }
    }

    /// A writer that drops everything (no usable dir).
    pub fn disabled() -> Self {
        let (tx, _) = sync_channel(0);
        let progress = Arc::new(Progress::default());
        progress.die();
        Self { tx, progress }
    }

    /// Queue one row; dropped when the queue is full or the store is gone.
    pub fn send(&self, row: Row) {
        if self.progress.dead.load(Ordering::Relaxed) {
            return;
        }
        self.progress.sent.fetch_add(1, Ordering::Relaxed);
        if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) = self.tx.try_send(row) {
            self.progress.finish(1);
        }
    }

    /// Wait until every row queued so far is written or dropped, for at
    /// most `limit`. True when it all landed in time.
    pub fn flush(&self, limit: Duration) -> bool {
        let p = &self.progress;
        let target = p.sent.load(Ordering::Relaxed);
        let deadline = Instant::now() + limit;
        let mut done = p.done.lock().unwrap_or_else(PoisonError::into_inner);
        while *done < target && !p.dead.load(Ordering::Relaxed) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            done = p
                .changed
                .wait_timeout(done, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        !p.dead.load(Ordering::Relaxed)
    }
}

fn drain(mut store: Store, rx: &Receiver<Row>, progress: &Progress) {
    let mut batch = Vec::with_capacity(BATCH);
    while let Ok(first) = rx.recv() {
        batch.push(first);
        batch.extend(rx.try_iter().take(BATCH - 1));
        // A failed batch (lock held past the busy timeout, IO, corruption)
        // is dropped; the next one tries again.
        let _ = store.record(&batch);
        progress.finish(batch.len() as u64);
        batch.clear();
    }
}
