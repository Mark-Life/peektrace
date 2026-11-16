//! Settings, capabilities and server info state, the save toast, the theme
//! pick and the shortcuts dialog. A settings reply re-seeds the form only
//! when the file's mtime changed, so edits survive a reload of the same file.

use peektrace_protocol::{
    AgentId, ApiError, Capability, ServerInfo, SettingsResult, SettingsUpdate,
};
use rune::TextEdit;
use rune::prefs::Prefs;

use super::{Request, Route, Ui};
use crate::model::settings::{self as rules, NATIVE, copy, saved_note, wire_error_message};
use crate::model::theme_mode::{Appearance, ThemeMode, ThemePress, next_theme_press};
use crate::source::LoadError;

/// A toast stays this long, in seconds.
pub const TOAST_S: f64 = 4.0;
/// The prefs key and app id the theme pick is kept under.
pub const THEME_KEY: &str = "theme";
pub const APP_ID: &str = "com.mark-life.peektrace";

/// One editable root row.
#[derive(Clone, Default, PartialEq)]
pub struct FormRow {
    pub path: TextEdit,
    pub label: TextEdit,
}

#[derive(Default)]
pub struct SettingsState {
    /// The snapshot the form was seeded from.
    pub loaded: Option<SettingsResult>,
    /// Rows per agent, in `AGENT_ORDER`.
    pub rows: [Vec<FormRow>; 4],
    pub busy: bool,
    pub conflict: bool,
    pub error: Option<LoadError>,
    pub retrying: bool,
    pub rid: u64,
    pub save_rid: u64,
    /// Edits aimed at a row removed meanwhile land here.
    scratch: TextEdit,
}

impl SettingsState {
    fn seed(&mut self, r: &SettingsResult) {
        self.rows = rules::seed_rows(&r.settings).map(|list| {
            list.into_iter()
                .map(|(p, l)| FormRow {
                    path: TextEdit::new(&p),
                    label: TextEdit::new(&l),
                })
                .collect()
        });
        self.conflict = false;
        self.busy = false;
    }

    /// The path (`label` false) or label editor of row `i` of agent slot `a`.
    pub fn edit(&mut self, a: usize, i: usize, label: bool) -> &mut TextEdit {
        match self.rows.get_mut(a).and_then(|r| r.get_mut(i)) {
            Some(row) if label => &mut row.label,
            Some(row) => &mut row.path,
            None => &mut self.scratch,
        }
    }

    /// The form as settings to save.
    pub fn assembled(&self) -> peektrace_protocol::PeektraceSettings {
        let rows = self.rows.each_ref().map(|list| {
            list.iter()
                .map(|r| (r.path.text.as_str(), r.label.text.as_str()))
                .collect::<Vec<_>>()
        });
        rules::assemble(&rows)
    }
}

#[derive(Debug, Default)]
pub struct CapsState {
    pub list: Option<Vec<Capability>>,
    pub error: Option<LoadError>,
    pub retrying: bool,
    pub rid: u64,
    /// The open cell: capability id and agent.
    pub selection: Option<(String, AgentId)>,
}

#[derive(Debug, Default)]
pub struct InfoState {
    pub value: Option<ServerInfo>,
    pub error: Option<LoadError>,
    pub retrying: bool,
    pub rid: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub title: String,
    pub description: Option<String>,
    pub error: bool,
    /// Clock second it hides at.
    pub until: f64,
}

/// The index of `agent` in the form's slots.
pub fn slot(agent: AgentId) -> usize {
    rules::AGENT_ORDER
        .iter()
        .position(|a| *a == agent)
        .unwrap_or(0)
}

impl Ui {
    /// Show `route`, queueing what it needs: entering Settings reloads the
    /// file and the scanned roots, staying on it keeps the edits; the static
    /// matrix loads once. Leaving Stats closes its sheet and resets its
    /// toggles.
    pub fn go(&mut self, route: Route) {
        let entering = self.route != route;
        if self.route == Route::Stats && entering {
            self.leave_stats();
        }
        self.route = route;
        match route {
            Route::Settings if entering => {
                self.request_settings();
                self.request_server_info();
            }
            Route::Capabilities => {
                self.caps.selection = None;
                if self.caps.list.is_none() {
                    self.request_capabilities();
                }
            }
            Route::Memory => self.enter_memory(),
            _ => {}
        }
    }

    pub fn request_settings(&mut self) {
        self.settings.rid += 1;
        self.pending.push(Request::Settings {
            rid: self.settings.rid,
        });
    }

    pub fn request_server_info(&mut self) {
        self.info.rid += 1;
        self.pending
            .push(Request::ServerInfo { rid: self.info.rid });
    }

    pub fn request_capabilities(&mut self) {
        self.caps.rid += 1;
        self.pending
            .push(Request::Capabilities { rid: self.caps.rid });
    }

    pub fn retry_settings(&mut self) {
        self.settings.retrying = true;
        self.request_settings();
    }

    pub fn retry_server_info(&mut self) {
        self.info.retrying = true;
        self.request_server_info();
    }

    pub fn retry_capabilities(&mut self) {
        self.caps.retrying = true;
        self.request_capabilities();
    }

    /// Land the settings file; a new mtime (or a first load) re-seeds the form.
    pub fn apply_settings(&mut self, rid: u64, result: Result<SettingsResult, LoadError>) -> bool {
        if rid != self.settings.rid {
            return false;
        }
        self.settings.retrying = false;
        match result {
            Ok(r) => {
                let fresh = self
                    .settings
                    .loaded
                    .as_ref()
                    .is_none_or(|l| l.mtime_ms != r.mtime_ms);
                if fresh {
                    self.settings.seed(&r);
                }
                self.settings.loaded = Some(r);
                self.settings.error = None;
            }
            Err(e) => self.settings.error = Some(e),
        }
        true
    }

    /// Land the scanned roots; a failure keeps an earlier answer on screen.
    pub fn apply_server_info(&mut self, rid: u64, result: Result<ServerInfo, LoadError>) -> bool {
        if rid != self.info.rid {
            return false;
        }
        self.info.retrying = false;
        match result {
            Ok(v) => {
                self.info.value = Some(v);
                self.info.error = None;
            }
            Err(e) => self.info.error = Some(e),
        }
        true
    }

    pub fn apply_capabilities(
        &mut self,
        rid: u64,
        result: Result<Vec<Capability>, LoadError>,
    ) -> bool {
        if rid != self.caps.rid {
            return false;
        }
        self.caps.retrying = false;
        match result {
            Ok(list) => {
                self.caps.list = Some(list);
                self.caps.error = None;
            }
            Err(e) => self.caps.error = Some(e),
        }
        true
    }

    /// Whether the server refuses writes.
    pub fn read_only(&self) -> bool {
        self.info.value.as_ref().is_some_and(|v| v.read_only)
    }

    pub fn add_root(&mut self, agent: AgentId) {
        if let Some(list) = self.settings.rows.get_mut(slot(agent)) {
            list.push(FormRow::default());
        }
    }

    pub fn remove_root(&mut self, agent: AgentId, i: usize) {
        if let Some(list) = self.settings.rows.get_mut(slot(agent))
            && i < list.len()
        {
            list.remove(i);
        }
    }

    /// Save the form; `guard` compares with the loaded mtime first,
    /// without it the save overwrites whatever is on disk.
    pub fn save_settings(&mut self, guard: bool) {
        if self.settings.busy {
            return;
        }
        #[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
        let mtime = self.settings.loaded.as_ref().map_or(0, |l| l.mtime_ms) as f64;
        self.settings.busy = true;
        self.settings.save_rid += 1;
        self.pending.push(Request::SaveSettings {
            rid: self.settings.save_rid,
            update: SettingsUpdate {
                settings: self.settings.assembled(),
                expected_mtime: guard.then_some(mtime),
            },
        });
    }

    /// Land a save at `clock` seconds: success toasts and reloads the file,
    /// a changed file shows the conflict and keeps the edits.
    pub fn apply_save(
        &mut self,
        rid: u64,
        result: Result<SettingsResult, LoadError>,
        clock: f64,
    ) -> bool {
        if rid != self.settings.save_rid {
            return false;
        }
        self.settings.busy = false;
        match result {
            Ok(_) => {
                self.settings.conflict = false;
                self.show_toast(copy::SAVED, Some(saved_note(NATIVE)), false, clock);
                self.request_settings();
            }
            Err(LoadError::Api(ApiError::FileChangedError { .. })) => {
                self.settings.conflict = true;
            }
            Err(LoadError::Api(e)) => {
                self.show_toast(&wire_error_message(&e), None, true, clock);
            }
            Err(_) => self.show_toast(copy::SAVE_FAILED, None, true, clock),
        }
        true
    }

    pub fn show_toast(&mut self, title: &str, description: Option<&str>, error: bool, clock: f64) {
        self.toast = Some(Toast {
            title: title.to_owned(),
            description: description.map(str::to_owned),
            error,
            until: clock + TOAST_S,
        });
    }

    pub fn open_cell(&mut self, cap: &str, agent: AgentId) {
        self.caps.selection = Some((cap.to_owned(), agent));
    }

    /// A dialog owns the keys while it is open: the shortcuts dialog, a
    /// capability cell, the Stats drill sheet or a memory dialog.
    pub const fn dialog_open(&self) -> bool {
        self.help_open
            || self.caps.selection.is_some()
            || self.stats.drill.is_some()
            || self.memory.modal_open()
    }

    /// What the next theme press does, from the look on screen.
    pub fn theme_press(&self) -> ThemePress {
        next_theme_press(
            Some(Appearance::of_dark(self.is_dark())),
            Some(Appearance::of_dark(self.system_dark.get())),
        )
    }

    /// Flip the look on screen and keep the pick; landing on the OS look
    /// stores `system`.
    pub fn flip_theme(&mut self) {
        let press = self.theme_press();
        self.dark = press.store.dark();
        if let Err(e) = self.prefs.set(THEME_KEY, press.store.as_str()) {
            tracing::warn!(error = %e, "could not keep the theme");
        }
    }

    /// Take the stored theme pick from `prefs` and keep later picks there.
    pub fn use_prefs(&mut self, prefs: Prefs) {
        let stored: Option<String> = prefs.get(THEME_KEY);
        self.dark = ThemeMode::parse(stored.as_deref().unwrap_or_default()).dark();
        self.prefs = prefs;
    }
}
