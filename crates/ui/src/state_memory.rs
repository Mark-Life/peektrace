//! Memory section state: the vault load and its live refresh, filters,
//! per-vault table state, the create, edit and delete dialogs, and toasts.
//! The transitions are `Ui` methods, so `state.rs` only holds the field.

use std::collections::{HashMap, HashSet};

use peektrace_protocol::view::memory::{AllVaultsView, EntryView};
use rune::TextEdit;

use crate::model::memory::{
    self as m, ALL, ProjectOption, Sort, Verdict, crud_cells, crud_verdict,
};
use crate::source::LoadError;
use crate::source::memory::{MemoryData, MemoryWrite};
use crate::state::{Request, Route, Ui};

mod run;
mod write;

pub use run::*;

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
    /// Bumped each time loaded data lands; what drawn caches key on.
    pub rev: u64,
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
            rev: 0,
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
                mem.rev += 1;
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

    /// Open a memory from a link or a graph node.
    pub fn navigate_memory(&mut self, vault: &str, slug: &str) {
        self.memory
            .expanded
            .insert(vault.to_owned(), slug.to_owned());
    }
}
