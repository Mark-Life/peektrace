//! Runs backend jobs off the draw loop: one thread per call, at most one
//! call per slot at a time; a newer job for a busy slot waits and replaces
//! any job already waiting there. A call that panics replies with an
//! error, so its slot frees up.

use std::any::Any;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::mpsc::Sender;

use crate::backend::{Backend, Job, Reply, Request, Response, Slot};

fn panic_text(payload: &(dyn Any + Send)) -> String {
    let what = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown cause");
    format!("internal error: {what}")
}

/// Run one call; a panic becomes the slot's error response.
pub fn call_guarded(backend: &dyn Backend, req: &Request) -> Response {
    catch_unwind(AssertUnwindSafe(|| backend.call(req)))
        .unwrap_or_else(|p| Response::failed(req.slot(), panic_text(p.as_ref())))
}

pub struct Dispatcher {
    backend: Arc<dyn Backend>,
    tx: Sender<Reply>,
    running: HashMap<Slot, u64>,
    queued: HashMap<Slot, Job>,
}

impl Dispatcher {
    pub fn new(backend: Arc<dyn Backend>, tx: Sender<Reply>) -> Self {
        Self {
            backend,
            tx,
            running: HashMap::new(),
            queued: HashMap::new(),
        }
    }

    pub fn submit(&mut self, job: Job) {
        let slot = job.req.slot();
        if self.running.contains_key(&slot) {
            self.queued.insert(slot, job);
            return;
        }
        self.spawn(job);
    }

    fn spawn(&mut self, job: Job) {
        let slot = job.req.slot();
        self.running.insert(slot, job.id);
        let backend = Arc::clone(&self.backend);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let resp = call_guarded(backend.as_ref(), &job.req);
            let _ = tx.send(Reply {
                id: job.id,
                slot,
                resp,
            });
        });
    }

    /// Note a finished call and start the slot's waiting job, if any.
    pub fn done(&mut self, r: &Reply) {
        if self.running.get(&r.slot) == Some(&r.id) {
            self.running.remove(&r.slot);
            if let Some(next) = self.queued.remove(&r.slot) {
                self.spawn(next);
            }
        }
    }
}

/// Run every queued job inline until none are left; for tests and
/// snapshots, where order must be fixed.
pub fn settle(app: &mut crate::App, backend: &dyn Backend) {
    loop {
        let jobs = app.take_jobs();
        if jobs.is_empty() {
            return;
        }
        for job in jobs {
            let slot = job.req.slot();
            let resp = backend.call(&job.req);
            app.reply(Reply {
                id: job.id,
                slot,
                resp,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    struct PanicsOnce(AtomicUsize);

    impl Backend for PanicsOnce {
        fn call(&self, _: &Request) -> Response {
            assert!(self.0.fetch_add(1, Ordering::SeqCst) > 0, "boom");
            Response::Sessions(Ok(Vec::new()))
        }
    }

    #[test]
    fn a_panicking_call_replies_with_an_error_and_frees_its_slot() {
        let (tx, rx) = mpsc::channel();
        let mut d = Dispatcher::new(Arc::new(PanicsOnce(AtomicUsize::new(0))), tx);
        d.submit(Job {
            id: 1,
            req: Request::Sessions,
        });
        d.submit(Job {
            id: 2,
            req: Request::Sessions,
        });
        let wait = Duration::from_secs(5);
        let first = rx.recv_timeout(wait).expect("the panicked job replies");
        assert_eq!(first.id, 1);
        assert!(
            matches!(&first.resp, Response::Sessions(Err(e)) if e.contains("boom")),
            "{:?}",
            first.resp
        );
        d.done(&first);
        let second = rx.recv_timeout(wait).expect("the queued job runs");
        assert_eq!(second.id, 2);
        assert!(matches!(second.resp, Response::Sessions(Ok(_))));
    }
}
