//! Memory section state: the vault load and its live refresh, filters,
//! per-vault table state, the create, edit and delete dialogs, and toasts.
//! The transitions are `Ui` methods, so `state.rs` only holds the field.

use std::collections::{HashMap, HashSet};

use peektrace_protocol::view::memory::{AllVaultsView, EntryView};
use peektrace_protocol::{ApiError, MemoryCreateBody, MemoryFrontmatterPatch};
use rune::TextEdit;

use crate::model::memory::{
    self as m, ALL, ProjectOption, Sort, Verdict, crud_cells, crud_verdict, deleted_toast,
    wire_error_message,
};
use crate::source::LoadError;
use crate::source::memory::{MemoryData, MemoryDone, MemoryWrite, expected_mtime};
use crate::state::{Request, Route, Ui};

/// How long a toast stays, in seconds.
pub const TOAST_S: f64 = 4.0;

/// A memory load or write the app layer runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryRequest {
    Load { rid: u64 },
    Write { id: u64, op: MemoryWrite },
}

/// Where a new memory goes: one vault, or picked in the dialog.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CreateTarget {
    Fixed(String),
    Pick,
}

/// A create dialog's fields; they outlive the dialog, as the inspector's do.
#[derive(Debug, Clone)]
pub struct Draft {
    pub name: TextEdit,
    pub description: TextEdit,
    pub kind: String,
    pub body: TextEdit,
    pub picked: Option<ProjectOption>,
    pub error: Option<String>,
    pub busy: bool,
    pub kind_open: bool,
    pub project_open: bool,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            name: TextEdit::new(""),
            description: TextEdit::new(""),
            kind: "project".to_owned(),
            body: TextEdit::new(""),
            picked: None,
            error: None,
            busy: false,
            kind_open: false,
            project_open: false,
        }
    }
}

/// The edit dialog: a snapshot of the entry, never refreshed, so the
/// compare-and-swap catches outside edits.
#[derive(Debug, Clone)]
pub struct EditForm {
    pub project: String,
    pub entry: EntryView,
    pub description: TextEdit,
    pub kind: String,
    pub body: TextEdit,
    pub conflict: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub kind_open: bool,
}

#[derive(Debug, Clone)]
pub struct DeleteForm {
    pub project: String,
    pub entry: EntryView,
    pub busy: bool,
}

#[derive(Debug, Clone)]
pub enum Dialog {
    Create(CreateTarget),
    Edit(Box<EditForm>),
    Delete(Box<DeleteForm>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub title: String,
    pub description: Option<String>,
    /// Clock seconds it goes at.
    pub due: f64,
}

/// One vault's table state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VaultUi {
    pub sort: Sort,
}

/// The write in flight and what it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Inflight {
    Create {
        target: CreateTarget,
        project: String,
        name: String,
    },
    Edit {
        slug: String,
    },
    Delete {
        slug: String,
    },
}

#[derive(Debug, Clone)]
#[expect(clippy::struct_excessive_bools, reason = "independent view flags")]
pub struct MemoryUi {
    pub data: Option<MemoryData>,
    pub error: Option<LoadError>,
    pub rid: u64,
    pub loading: bool,
    /// The memory version last seen; `None` until the first event.
    pub seen: Option<u64>,
    /// A change landed while the section was off screen or paused.
    pub stale: bool,
    pub query: TextEdit,
    pub ty: String,
    pub scope: String,
    pub type_open: bool,
    pub scope_open: bool,
    /// Collapsed vaults; every other one is open.
    pub closed: HashSet<String>,
    pub vaults: HashMap<String, VaultUi>,
    /// The expanded row per vault, shared by the table and the graph.
    pub expanded: HashMap<String, String>,
    pub dialog: Option<Dialog>,
    pub drafts: HashMap<CreateTarget, Draft>,
    /// A created vault whose trigger takes focus once it shows.
    pub focus: Option<String>,
    pub toasts: Vec<Toast>,
    toast_id: u64,
    write_id: u64,
    inflight: Option<(u64, Inflight)>,
    /// Takes keys meant for an edit field once its dialog has gone.
    spare: TextEdit,
}

impl Default for MemoryUi {
    fn default() -> Self {
        Self {
            data: None,
            error: None,
            rid: 0,
            loading: false,
            seen: None,
            stale: false,
            query: TextEdit::new(""),
            ty: ALL.to_owned(),
            scope: ALL.to_owned(),
            type_open: false,
            scope_open: false,
            closed: HashSet::new(),
            vaults: HashMap::new(),
            expanded: HashMap::new(),
            dialog: None,
            drafts: HashMap::new(),
            focus: None,
            toasts: Vec::new(),
            toast_id: 0,
            write_id: 0,
            inflight: None,
            spare: TextEdit::new(""),
        }
    }
}

impl MemoryUi {
    pub const fn modal_open(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn verdict(&self) -> Option<Verdict> {
        self.data
            .as_ref()
            .map(|d| crud_verdict(&crud_cells(&d.caps), d.read_only))
    }

    pub fn can_write(&self) -> bool {
        self.verdict().is_some_and(|v| v.can_write())
    }

    pub fn all(&self) -> Option<&AllVaultsView> {
        self.data.as_ref().map(|d| &d.all)
    }

    pub fn draft(&self, t: &CreateTarget) -> Draft {
        self.drafts.get(t).cloned().unwrap_or_default()
    }

    pub fn draft_mut(&mut self, t: &CreateTarget) -> &mut Draft {
        self.drafts.entry(t.clone()).or_default()
    }

    pub fn edit_mut(&mut self) -> Option<&mut EditForm> {
        match &mut self.dialog {
            Some(Dialog::Edit(f)) => Some(f),
            _ => None,
        }
    }

    /// The edit dialog's description or body field.
    pub fn edit_text(&mut self, body: bool) -> &mut TextEdit {
        match &mut self.dialog {
            Some(Dialog::Edit(f)) => {
                if body {
                    &mut f.body
                } else {
                    &mut f.description
                }
            }
            _ => &mut self.spare,
        }
    }

    fn toast(&mut self, kind: ToastKind, title: String, description: Option<String>, clock: f64) {
        self.toast_id += 1;
        self.toasts.push(Toast {
            id: self.toast_id,
            kind,
            title,
            description,
            due: clock + TOAST_S,
        });
    }

    /// Close a create dialog: only the pick and the error go.
    pub fn close_create(&mut self, t: &CreateTarget) {
        let d = self.draft_mut(t);
        d.picked = None;
        d.error = None;
        d.kind_open = false;
        d.project_open = false;
        if matches!(&self.dialog, Some(Dialog::Create(x)) if x == t) {
            self.dialog = None;
        }
    }

    pub fn close_dialog(&mut self) {
        match self.dialog.clone() {
            Some(Dialog::Create(t)) => self.close_create(&t),
            _ => self.dialog = None,
        }
    }

    /// A row press: the same row closes, another opens.
    pub fn toggle_row(&mut self, vault: &str, entry: &str) {
        if self.expanded.get(vault).is_some_and(|e| e == entry) {
            self.expanded.remove(vault);
        } else {
            self.expanded.insert(vault.to_owned(), entry.to_owned());
        }
    }

    pub fn toggle_vault(&mut self, slug: &str) {
        if !self.closed.remove(slug) {
            self.closed.insert(slug.to_owned());
        }
    }

    pub fn sort_of(&self, vault: &str) -> Sort {
        self.vaults.get(vault).map(|v| v.sort).unwrap_or_default()
    }

    pub fn press_sort(&mut self, vault: &str, key: m::SortKey) {
        let v = self.vaults.entry(vault.to_owned()).or_default();
        v.sort = v.sort.pressed(key);
    }

    /// Drop toasts that are due; true when one went.
    pub fn expire(&mut self, clock: f64) -> bool {
        let before = self.toasts.len();
        self.toasts.retain(|t| t.due > clock);
        self.toasts.len() != before
    }

    pub fn next_due(&self) -> Option<f64> {
        self.toasts.iter().map(|t| t.due).reduce(f64::min)
    }
}

fn error_text(e: &LoadError, fallback: &str) -> String {
    match e {
        LoadError::Api(a) => wire_error_message(a),
        _ => fallback.to_owned(),
    }
}

impl Ui {
    /// On Memory: load it when it has no data, the last load failed, or
    /// memory changed meanwhile.
    pub fn enter_memory(&mut self) {
        let mem = &self.memory;
        let due = mem.data.is_none() || mem.error.is_some() || mem.stale;
        if due && !mem.loading {
            self.request_memory();
        }
    }

    pub fn request_memory(&mut self) {
        let mem = &mut self.memory;
        mem.rid += 1;
        mem.loading = true;
        mem.stale = false;
        self.pending
            .push(Request::Memory(MemoryRequest::Load { rid: mem.rid }));
    }

    /// A memory watch version. The first only sets the baseline, a lower
    /// one (a restarted server) resets it; a bump reloads at once on the
    /// section, else marks it stale.
    pub fn on_memory_version(&mut self, v: u64) {
        let mem = &mut self.memory;
        let bumped = mem.seen.is_some_and(|s| v > s);
        mem.seen = Some(v);
        if !bumped {
            return;
        }
        if self.paused || self.route != Route::Memory || mem.data.is_none() {
            mem.stale = true;
        } else {
            self.request_memory();
        }
    }

    /// The live link came back: what changed meanwhile is unknown.
    pub fn on_memory_reconnect(&mut self) {
        if self.route == Route::Memory && !self.paused {
            self.request_memory();
        } else {
            self.memory.stale = true;
        }
    }

    /// Timers: due toasts go; a change held while paused loads on resume.
    pub fn memory_tick(&mut self, clock: f64) -> bool {
        let mut changed = self.memory.expire(clock);
        let resume = self.memory.stale
            && !self.paused
            && self.route == Route::Memory
            && self.memory.data.is_some()
            && !self.memory.loading;
        if resume {
            self.request_memory();
            changed = true;
        }
        changed
    }

    /// Land a load; an older request's reply changes nothing. A failure
    /// keeps the vaults on screen. Returns the vault key to focus, once.
    pub fn apply_memory(
        &mut self,
        rid: u64,
        result: Result<MemoryData, LoadError>,
    ) -> Option<String> {
        let mem = &mut self.memory;
        if rid != mem.rid {
            return None;
        }
        mem.loading = false;
        match result {
            Ok(d) => {
                mem.data = Some(d);
                mem.error = None;
            }
            Err(e) => mem.error = Some(e),
        }
        let slug = mem.focus.clone()?;
        let shown = mem.data.as_ref().is_some_and(|d| {
            m::visible_vaults(&d.all, &mem.scope, &mem.query.text, &mem.ty)
                .iter()
                .any(|v| v.vault.slug == slug)
        });
        shown.then(|| {
            mem.focus = None;
            crate::views::memory::vault_key(&slug)
        })
    }

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

    /// Open a memory from a link or a graph node.
    pub fn navigate_memory(&mut self, vault: &str, slug: &str) {
        self.memory
            .expanded
            .insert(vault.to_owned(), slug.to_owned());
    }
}

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
