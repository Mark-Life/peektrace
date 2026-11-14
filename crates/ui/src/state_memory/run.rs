//! Running memory requests, and the headless scenes.

use peektrace_protocol::view::memory::EntryView;
use rune::TextEdit;

use super::{CreateTarget, Dialog, MemoryRequest, TOAST_S};
use crate::model::memory::{self as m, Sort};
use crate::state::{Route, Ui};

/// Hand a memory request to the source; replies land through the handle.
pub fn dispatch(req: MemoryRequest, io: &crate::state::Io) {
    let h = io.handle.clone();
    match req {
        MemoryRequest::Load { rid } => io.source.memory(Box::new(move |res| {
            h.update(move |s, tx| {
                s.syncing = s.syncing.saturating_sub(1);
                if let Some(key) = s.apply_memory(rid, res) {
                    tx.focus(key, true);
                }
            });
        })),
        MemoryRequest::Write { id, op } => io.source.memory_write(
            op,
            Box::new(move |res| {
                h.update(move |s, tx| {
                    s.syncing = s.syncing.saturating_sub(1);
                    let clock = tx.clock();
                    if s.apply_memory_write(id, res, clock) {
                        tx.wake_at(clock + TOAST_S);
                    }
                    if !s.pending.is_empty() {
                        tx.wake_after(std::time::Duration::ZERO);
                    }
                });
            }),
        ),
    }
}

/// Run a memory request at once (headless runs, scenes and tests).
pub fn load_now(s: &mut Ui, source: &dyn crate::source::DataSource, req: MemoryRequest) {
    match req {
        MemoryRequest::Load { rid } => {
            if let Some(r) = source.memory_now() {
                s.apply_memory(rid, r);
            }
        }
        MemoryRequest::Write { id, op } => {
            if let Some(r) = source.memory_write_now(&op) {
                s.apply_memory_write(id, r, 0.0);
            }
        }
    }
}

/// Scene: the Memory section, loaded.
pub fn scene(s: &mut Ui) {
    s.go(Route::Memory);
    crate::load_now(s);
}

/// Scene: a query and a type that narrow the vaults.
pub fn scene_filtered(s: &mut Ui) {
    scene(s);
    s.memory.query = TextEdit::new("note");
    "user".clone_into(&mut s.memory.ty);
}

/// Scene: the first vault's first entry with links (else its first), open.
pub fn scene_entry(s: &mut Ui) {
    scene(s);
    let pick = s.memory.all().and_then(|a| {
        let v = a.vaults.iter().find(|v| !v.entries.is_empty())?;
        let refs: Vec<&EntryView> = v.entries.iter().collect();
        let sorted = m::sort_entries(&refs, Sort::default());
        let e = sorted
            .iter()
            .find(|e| !e.links.is_empty())
            .or_else(|| sorted.first())?;
        Some((v.slug.clone(), e.slug.clone()))
    });
    if let Some((vault, slug)) = pick {
        s.navigate_memory(&vault, &slug);
    }
}

/// Scene: the "New memory in…" dialog.
pub fn scene_create(s: &mut Ui) {
    scene(s);
    s.memory.dialog = Some(Dialog::Create(CreateTarget::Pick));
}
