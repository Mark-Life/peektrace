//! Memory writes: sending the dialogs and landing their answers.

use peektrace_protocol::view::memory::EntryView;
use peektrace_protocol::{ApiError, MemoryCreateBody, MemoryFrontmatterPatch};
use rune::TextEdit;

use super::{CreateTarget, DeleteForm, Dialog, EditForm, Inflight, MemoryRequest, ToastKind};
use crate::model::memory::{ALL, deleted_toast, wire_error_message};
use crate::source::LoadError;
use crate::source::memory::{MemoryDone, MemoryWrite, expected_mtime};
use crate::state::{Request, Ui};

fn error_text(e: &LoadError, fallback: &str) -> String {
    match e {
        LoadError::Api(a) => wire_error_message(a),
        _ => fallback.to_owned(),
    }
}

impl Ui {
    fn write(&mut self, op: MemoryWrite, what: Inflight) {
        let mem = &mut self.memory;
        mem.write_id += 1;
        mem.inflight = Some((mem.write_id, what));
        self.pending.push(Request::Memory(MemoryRequest::Write {
            id: mem.write_id,
            op,
        }));
    }

    /// Send a create dialog; no client-side name check, the core error
    /// shows inline.
    pub fn submit_create(&mut self, t: &CreateTarget) {
        let d = self.memory.draft(t);
        let project = match t {
            CreateTarget::Fixed(slug) => Some(slug.clone()),
            CreateTarget::Pick => d.picked.as_ref().map(|p| p.slug.clone()),
        };
        let Some(project) = project.filter(|_| !d.busy && !d.name.text.is_empty()) else {
            return;
        };
        let body = MemoryCreateBody {
            name: d.name.text.clone(),
            description: d.description.text.clone(),
            kind: d.kind.clone(),
            body: d.body.text.clone(),
        };
        let draft = self.memory.draft_mut(t);
        draft.busy = true;
        draft.error = None;
        let what = Inflight::Create {
            target: t.clone(),
            project: project.clone(),
            name: body.name.clone(),
        };
        self.write(MemoryWrite::Create { project, body }, what);
    }

    pub fn open_edit(&mut self, project: &str, entry: &EntryView) {
        self.memory.dialog = Some(Dialog::Edit(Box::new(EditForm {
            project: project.to_owned(),
            entry: entry.clone(),
            description: TextEdit::new(entry.description.as_deref().unwrap_or_default()),
            kind: entry.kind.clone().unwrap_or_else(|| "project".to_owned()),
            body: TextEdit::new(&entry.body),
            conflict: false,
            busy: false,
            error: None,
            kind_open: false,
        })));
    }

    /// Save the edit; `guard` sends the snapshot's mtime. The patch always
    /// carries description and type, as the inspector's does.
    pub fn submit_edit(&mut self, guard: bool) {
        let Some(f) = self.memory.edit_mut() else {
            return;
        };
        if f.busy {
            return;
        }
        f.busy = true;
        f.error = None;
        let op = MemoryWrite::Update {
            project: f.project.clone(),
            name: f.entry.slug.clone(),
            frontmatter: Some(MemoryFrontmatterPatch {
                name: None,
                description: Some(f.description.text.clone()),
                kind: Some(f.kind.clone()),
            }),
            body: Some(f.body.text.clone()),
            expected_mtime: guard.then(|| expected_mtime(f.entry.mtime_ms)),
        };
        let slug = f.entry.slug.clone();
        self.write(op, Inflight::Edit { slug });
    }

    pub fn open_delete(&mut self, project: &str, entry: &EntryView) {
        self.memory.dialog = Some(Dialog::Delete(Box::new(DeleteForm {
            project: project.to_owned(),
            entry: entry.clone(),
            busy: false,
        })));
    }

    pub fn submit_delete(&mut self) {
        let Some(Dialog::Delete(f)) = &mut self.memory.dialog else {
            return;
        };
        if f.busy {
            return;
        }
        f.busy = true;
        let op = MemoryWrite::Delete {
            project: f.project.clone(),
            name: f.entry.slug.clone(),
        };
        let slug = f.entry.slug.clone();
        self.write(op, Inflight::Delete { slug });
    }

    /// Land a write at `clock` seconds: toast, close and reload on success;
    /// the error inline (create, edit), as conflict mode (a changed file)
    /// or as a toast (delete). True when a toast was added.
    pub fn apply_memory_write(
        &mut self,
        id: u64,
        result: Result<MemoryDone, LoadError>,
        clock: f64,
    ) -> bool {
        let mem = &mut self.memory;
        let Some((_, what)) = mem.inflight.clone().filter(|(at, _)| *at == id) else {
            return false;
        };
        mem.inflight = None;
        match (what, result) {
            (
                Inflight::Create {
                    target,
                    project,
                    name,
                },
                Ok(_),
            ) => {
                mem.toast(ToastKind::Success, format!("Created {name}"), None, clock);
                mem.close_create(&target);
                let d = mem.draft_mut(&target);
                d.busy = false;
                d.name = TextEdit::new("");
                d.description = TextEdit::new("");
                d.body = TextEdit::new("");
                if target == CreateTarget::Pick {
                    mem.query = TextEdit::new("");
                    ALL.clone_into(&mut mem.ty);
                    mem.closed.remove(&project);
                    if mem.scope != ALL && mem.scope != project {
                        mem.scope.clone_from(&project);
                    }
                    mem.focus = Some(project);
                }
                self.request_memory();
                true
            }
            (Inflight::Create { target, .. }, Err(e)) => {
                let d = mem.draft_mut(&target);
                d.busy = false;
                d.error = Some(error_text(&e, "Create failed."));
                false
            }
            (Inflight::Edit { slug }, Ok(_)) => {
                mem.toast(ToastKind::Success, format!("Saved {slug}"), None, clock);
                if matches!(mem.dialog, Some(Dialog::Edit(_))) {
                    mem.dialog = None;
                }
                self.request_memory();
                true
            }
            (Inflight::Edit { .. }, Err(e)) => {
                if let Some(f) = mem.edit_mut() {
                    f.busy = false;
                    match &e {
                        LoadError::Api(ApiError::FileChangedError { .. }) => f.conflict = true,
                        _ => f.error = Some(error_text(&e, "Save failed.")),
                    }
                }
                false
            }
            (Inflight::Delete { slug }, Ok(done)) => {
                let (title, description, warns) = match &done {
                    MemoryDone::Deleted(d) => deleted_toast(&slug, d),
                    MemoryDone::Entry(_) => (format!("Deleted {slug}"), None, false),
                };
                let kind = if warns {
                    ToastKind::Warning
                } else {
                    ToastKind::Success
                };
                mem.toast(kind, title, description, clock);
                if matches!(mem.dialog, Some(Dialog::Delete(_))) {
                    mem.dialog = None;
                }
                self.request_memory();
                true
            }
            (Inflight::Delete { .. }, Err(e)) => {
                if let Some(Dialog::Delete(f)) = &mut mem.dialog {
                    f.busy = false;
                }
                let text = error_text(&e, "Delete failed.");
                mem.toast(ToastKind::Error, text, None, clock);
                true
            }
        }
    }

    /// Reload after a conflict: the edits go, the dialog closes.
    pub fn reload_from_disk(&mut self) {
        self.memory.dialog = None;
        self.request_memory();
    }
}
