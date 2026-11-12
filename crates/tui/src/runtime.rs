//! Runs backend jobs off the draw loop: one thread per call, at most one
//! call per slot at a time; a newer job for a busy slot waits and replaces
//! any job already waiting there.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use crate::backend::{Backend, Job, Reply, Slot};

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
            let resp = backend.call(&job.req);
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
