//! The Settings page driven through Rune's `Runtime` on temp homes: the
//! default roots, read-only mode, add, remove, save, the compare-and-swap
//! conflict and failed loads. Never the real `~/.peektrace`.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use common::home;
use peektrace_protocol::view::{AnalyzedSession, SessionListingView};
use peektrace_protocol::{
    AgentId, AgentScan, ApiError, Capability, READ_ONLY_REASON, ScannedRoot, ServerInfo,
    SettingsResult, SettingsUpdate,
};
use peektrace_ui::source::in_process::InProcess;
use peektrace_ui::source::{DataSource, LoadError, OnVersions, Reply};
use peektrace_ui::state::{Io, Request, Route, Ui};
use peektrace_ui::{load_now, views};
use rune::{App, Env, Key, KeyEvent, Mods, Runtime, TextEdit};

const SIZE: (f32, f32) = (1440.0, 900.0);
const HOME: &str = "/home/dev";

fn settings_file(home: &Path) -> PathBuf {
    home.join(".peektrace/settings.json")
}

fn source(home: &Path) -> InProcess {
    let dir = home.to_string_lossy().into_owned();
    let peektrace = format!("{dir}/.peektrace");
    InProcess::for_home(dir, move |k| {
        (k == "PEEKTRACE_DIR").then(|| peektrace.clone())
    })
}

fn state_on(source: Arc<dyn DataSource>) -> Ui {
    let handle = App::new(peektrace_ui::spec()).handle();
    let mut s = Ui::new(Some(Io { source, handle }), 0.0, true);
    s.go(Route::Settings);
    load_now(&mut s);
    s
}

fn state(home: &Path) -> Ui {
    state_on(Arc::new(source(home)))
}

fn runtime(s: Ui) -> Runtime<Ui> {
    let mut rt = Runtime::new(s, views::view, SIZE, Env::new(true, 1.0));
    rt.on_key = Some(peektrace_ui::on_key);
    settle(&mut rt);
    rt
}

fn settle(rt: &mut Runtime<Ui>) {
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
}

fn texts(rt: &Runtime<Ui>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter_map(|e| e.text.or(e.label))
        .collect()
}

fn has_text(rt: &Runtime<Ui>, needle: &str) -> bool {
    texts(rt).iter().any(|t| t.contains(needle))
}

fn count_key(rt: &Runtime<Ui>, key: &str) -> usize {
    rt.elements()
        .iter()
        .filter(|e| e.key.as_deref() == Some(key) && !e.exiting)
        .count()
}

fn has_key(rt: &Runtime<Ui>, key: &str) -> bool {
    rt.find(key).is_some()
}

fn click(rt: &mut Runtime<Ui>, key: &str) {
    let owned = key.to_owned();
    rt.update(move |_, tx| tx.scroll_into_view(owned));
    settle(rt);
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

fn press(rt: &mut Runtime<Ui>, key: Key) {
    rt.key(KeyEvent {
        key,
        mods: Mods::default(),
    });
    settle(rt);
}

/// Run what the page queued, as the app's poll would.
fn run_pending(rt: &mut Runtime<Ui>) {
    for _ in 0..3 {
        rt.update(|s, _| load_now(s));
    }
    settle(rt);
}

fn tester_info(read_only: bool) -> ServerInfo {
    let root = |default: bool, home: &str, path: &str, exists: bool| ScannedRoot {
        is_default: default,
        home: format!("{HOME}{home}"),
        path: format!("{HOME}{path}"),
        exists,
    };
    ServerInfo {
        read_only,
        user_home: HOME.into(),
        agents: vec![
            AgentScan {
                agent: AgentId::Claude,
                roots: vec![
                    root(true, "/.claude", "/.claude/projects", true),
                    root(false, "/work/.claude", "/work/.claude/projects", true),
                ],
            },
            AgentScan {
                agent: AgentId::Codex,
                roots: vec![root(true, "/.codex", "/.codex/sessions", false)],
            },
        ],
    }
}

/// The page over a fixed server answer, as the inspector's editor test renders it.
fn fixed_page(read_only: bool) -> Runtime<Ui> {
    let mut s = state(&home::temp_home("settings-fixed"));
    s.info.value = Some(tester_info(read_only));
    if let Some(l) = &mut s.settings.loaded {
        l.path = format!("{HOME}/.peektrace/settings.json");
    }
    runtime(s)
}

#[test]
fn defaults_show_as_config_dirs_with_home_as_tilde() {
    let rt = fixed_page(false);
    assert!(has_text(&rt, "~/.peektrace/settings.json"));
    assert!(texts(&rt).iter().any(|t| t == "~/.claude"));
    assert!(has_text(&rt, "scans ~/.claude/projects"));
    assert!(has_text(&rt, "scans ~/.codex/sessions"));
    assert!(!has_text(&rt, "~/work/.claude/projects"));
    assert_eq!(count_key(&rt, "settings-default-root"), 2);
    assert_eq!(count_key(&rt, "root-missing"), 1);
    assert!(!has_text(&rt, &format!("{HOME}/")));
    for agent in ["claude", "codex", "pi", "opencode"] {
        assert!(has_key(&rt, &format!("settings-agent-{agent}")), "{agent}");
    }
    assert_eq!(count_key(&rt, "settings-default-root"), 2);
}

#[test]
fn an_editable_server_leaves_the_form_enabled() {
    let mut rt = fixed_page(false);
    assert!(!has_key(&rt, "settings-readonly"));
    assert!(!has_key(&rt, "read-only-badge"));
    click(&mut rt, "settings-add-claude");
    assert_eq!(rt.state.settings.rows[0].len(), 1);
}

#[test]
fn a_read_only_server_disables_the_form_and_says_why() {
    let mut rt = fixed_page(true);
    assert!(has_key(&rt, "settings-readonly"));
    assert!(has_key(&rt, "read-only-badge"));
    assert!(has_text(&rt, "Read-only server"));
    assert!(has_text(
        &rt,
        "The server was started with --read-only, or on a non-loopback --host without --allow-write. Settings can't be saved from here."
    ));
    click(&mut rt, "settings-add-claude");
    click(&mut rt, "settings-save");
    assert!(rt.state.settings.rows[0].is_empty());
    assert_eq!(rt.state.pending, []);
    // The matrix link sits outside the form.
    click(&mut rt, "settings-capabilities");
    assert_eq!(rt.state.route, Route::Capabilities);
}

#[test]
fn add_type_remove_and_save_writes_the_trimmed_file() {
    let dir = home::temp_home("settings-save");
    let mut rt = runtime(state(&dir));
    assert_eq!(
        rt.state.settings.loaded.as_ref().map(|l| l.mtime_ms),
        Some(0)
    );
    let empty = texts(&rt)
        .iter()
        .filter(|t| *t == "No extra roots.")
        .count();
    assert_eq!(empty, 4);
    click(&mut rt, "settings-add-claude");
    assert!(has_key(&rt, "settings-path-claude-0"));
    assert!(has_key(&rt, "settings-label-claude-0"));
    click(&mut rt, "settings-remove-claude-0");
    assert!(rt.state.settings.rows[0].is_empty());
    click(&mut rt, "settings-add-claude");
    click(&mut rt, "settings-add-claude");
    rt.update(|s, _| {
        *s.settings.edit(0, 0, false) = TextEdit::new(" ~/w/.claude ");
        *s.settings.edit(0, 0, true) = TextEdit::new("  ");
    });
    settle(&mut rt);
    click(&mut rt, "settings-save");
    assert!(rt.state.settings.busy);
    assert!(has_text(&rt, "Saving…"));
    assert!(matches!(
        rt.state.pending.as_slice(),
        [Request::SaveSettings { update: SettingsUpdate { expected_mtime: Some(m), .. }, .. }] if *m == 0.0
    ));
    run_pending(&mut rt);
    let text = std::fs::read_to_string(settings_file(&dir)).unwrap_or_default();
    assert_eq!(
        text,
        "{\n  \"roots\": {\n    \"claude\": [\n      {\n        \"path\": \"~/w/.claude\"\n      }\n    ]\n  }\n}\n"
    );
    assert!(has_key(&rt, "toast"));
    assert!(has_text(&rt, "Settings saved"));
    assert!(has_text(
        &rt,
        "Restart Peektrace to apply to the session list."
    ));
    assert!(!rt.state.settings.busy);
    assert_ne!(
        rt.state.settings.loaded.as_ref().map(|l| l.mtime_ms),
        Some(0)
    );
    let rows: Vec<(String, String)> = rt.state.settings.rows[0]
        .iter()
        .map(|r| (r.path.text.clone(), r.label.text.clone()))
        .collect();
    assert_eq!(rows, [("~/w/.claude".to_owned(), String::new())]);
    // The toast hides on its own.
    rt.update(|s, tx| {
        s.tick(10.0, 0.0, tx);
    });
    settle(&mut rt);
    assert!(!has_key(&rt, "toast"));
}

fn touch(path: &Path, text: &str, secs_ahead: u64) {
    std::fs::write(path, text).expect("write");
    let t = SystemTime::now() + Duration::from_secs(secs_ahead);
    std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(t))
        .expect("set mtime");
}

#[test]
fn a_file_changed_after_load_shows_the_conflict_and_keeps_edits() {
    let dir = home::temp_home("settings-cas");
    let file = settings_file(&dir);
    home::write(&file, "{\"roots\":{\"codex\":[{\"path\":\"~/c\"}]}}\n");
    let mut rt = runtime(state(&dir));
    assert_eq!(rt.state.settings.rows[1].len(), 1);
    click(&mut rt, "settings-add-pi");
    rt.update(|s, _| *s.settings.edit(2, 0, false) = TextEdit::new("~/p"));
    touch(
        &file,
        "{\"roots\":{\"claude\":[{\"path\":\"~/other\"}]}}\n",
        60,
    );
    click(&mut rt, "settings-save");
    run_pending(&mut rt);
    assert!(rt.state.settings.conflict);
    assert!(has_key(&rt, "settings-cas-conflict"));
    assert!(has_text(&rt, "settings.json changed on disk"));
    assert_eq!(rt.state.settings.rows[2].len(), 1, "edits kept");

    click(&mut rt, "settings-cas-overwrite");
    run_pending(&mut rt);
    assert!(!has_key(&rt, "settings-cas-conflict"));
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(text.contains("~/p") && text.contains("~/c") && !text.contains("~/other"));

    rt.update(|s, _| *s.settings.edit(2, 0, false) = TextEdit::new("~/edited"));
    touch(
        &file,
        "{\"roots\":{\"claude\":[{\"path\":\"~/disk\"}]}}\n",
        120,
    );
    click(&mut rt, "settings-save");
    run_pending(&mut rt);
    assert!(has_key(&rt, "settings-cas-conflict"));
    click(&mut rt, "settings-cas-reload");
    run_pending(&mut rt);
    assert!(!has_key(&rt, "settings-cas-conflict"));
    assert_eq!(rt.state.settings.rows[0][0].path.text, "~/disk");
    assert!(
        rt.state.settings.rows[2].is_empty(),
        "reload discards edits"
    );
}

#[test]
fn a_file_deleted_after_load_is_a_missing_conflict_that_overwrite_recreates() {
    let dir = home::temp_home("settings-gone");
    let file = settings_file(&dir);
    home::write(&file, "{}\n");
    let mut rt = runtime(state(&dir));
    std::fs::remove_file(&file).expect("rm");
    click(&mut rt, "settings-add-claude");
    rt.update(|s, _| *s.settings.edit(0, 0, false) = TextEdit::new("~/a"));
    click(&mut rt, "settings-save");
    run_pending(&mut rt);
    assert!(rt.state.settings.conflict);
    click(&mut rt, "settings-cas-overwrite");
    run_pending(&mut rt);
    assert!(
        std::fs::read_to_string(&file)
            .unwrap_or_default()
            .contains("~/a")
    );
}

#[test]
fn edits_survive_leaving_while_the_file_is_unchanged() {
    let dir = home::temp_home("settings-leave");
    let mut rt = runtime(state(&dir));
    click(&mut rt, "settings-add-opencode");
    click(&mut rt, "nav-sessions");
    click(&mut rt, "nav-settings");
    run_pending(&mut rt);
    assert_eq!(rt.state.settings.rows[3].len(), 1);
}

#[test]
fn typing_shortcut_letters_in_a_field_types_them() {
    let dir = home::temp_home("settings-typing");
    let mut rt = runtime(state(&dir));
    click(&mut rt, "settings-add-claude");
    click(&mut rt, "settings-path-claude-0");
    let dark = rt.state.dark;
    for k in ["d", "?", "/", "j"] {
        press(&mut rt, Key::Text(k.into()));
    }
    assert_eq!(rt.state.settings.rows[0][0].path.text, "d?/j");
    assert_eq!(rt.state.dark, dark);
    assert!(!rt.state.help_open);
}

/// A source whose settings calls fail as told; the rest answer from `inner`.
struct Failing {
    inner: InProcess,
    load: Option<LoadError>,
    info: Option<LoadError>,
    save: Option<LoadError>,
}

impl DataSource for Failing {
    fn sessions(&self, reply: Reply<SessionListingView>) {
        self.inner.sessions(reply);
    }
    fn analysis(&self, id: String, parent: Option<String>, reply: Reply<AnalyzedSession>) {
        self.inner.analysis(id, parent, reply);
    }
    fn watch(&self, _on_versions: OnVersions) {}
    fn stats_report(
        &self,
        scope: peektrace_ui::model::stats::StatsScope,
        reply: Reply<peektrace_ui::source::StatsScreen>,
    ) {
        self.inner.stats_report(scope, reply);
    }
    fn stats_drill(
        &self,
        scope: peektrace_ui::model::stats::StatsScope,
        ask: peektrace_ui::model::stats::DrillAsk,
        reply: Reply<peektrace_protocol::stats::DrillView>,
    ) {
        self.inner.stats_drill(scope, ask, reply);
    }
    fn capabilities(&self, reply: Reply<Vec<Capability>>) {
        self.inner.capabilities(reply);
    }
    fn server_info(&self, reply: Reply<ServerInfo>) {
        self.inner.server_info(reply);
    }
    fn settings(&self, reply: Reply<SettingsResult>) {
        self.inner.settings(reply);
    }
    fn save_settings(&self, update: SettingsUpdate, reply: Reply<SettingsResult>) {
        self.inner.save_settings(update, reply);
    }
    fn server_info_now(&self) -> Option<Result<ServerInfo, LoadError>> {
        Some(self.info.clone().map_or_else(
            || {
                self.inner
                    .server_info_now()
                    .unwrap_or(Err(LoadError::Decode(String::new())))
            },
            Err,
        ))
    }
    fn settings_now(&self) -> Option<Result<SettingsResult, LoadError>> {
        Some(self.load.clone().map_or_else(
            || {
                self.inner
                    .settings_now()
                    .unwrap_or(Err(LoadError::Decode(String::new())))
            },
            Err,
        ))
    }
    fn save_settings_now(
        &self,
        update: SettingsUpdate,
    ) -> Option<Result<SettingsResult, LoadError>> {
        Some(self.save.clone().map_or_else(
            || {
                self.inner
                    .save_settings_now(update)
                    .unwrap_or(Err(LoadError::Decode(String::new())))
            },
            Err,
        ))
    }
}

fn failing(
    tag: &str,
    load: Option<LoadError>,
    info: Option<LoadError>,
    save: Option<LoadError>,
) -> Ui {
    state_on(Arc::new(Failing {
        inner: source(&home::temp_home(tag)),
        load,
        info,
        save,
    }))
}

#[test]
fn a_typed_save_failure_toasts_its_sentence() {
    let refused = LoadError::Api(ApiError::SettingsWriteError {
        path: "settings.json".into(),
        reason: READ_ONLY_REASON.into(),
    });
    let mut rt = runtime(failing("settings-refused", None, None, Some(refused)));
    click(&mut rt, "settings-save");
    run_pending(&mut rt);
    assert!(has_text(
        &rt,
        "Settings could not be saved (read-only mode is enabled; refusing to write)."
    ));
    assert!(rt.state.toast.as_ref().is_some_and(|t| t.error));
}

#[test]
fn an_untyped_save_failure_says_save_failed() {
    let broken = LoadError::Transport {
        status: Some(500),
        message: "boom".into(),
    };
    let mut rt = runtime(failing("settings-broken", None, None, Some(broken)));
    click(&mut rt, "settings-save");
    run_pending(&mut rt);
    assert!(has_text(&rt, "Save failed."));
}

#[test]
fn a_failed_load_offers_retry() {
    let gone = LoadError::Api(ApiError::NotFoundError {
        path: "/api/settings".into(),
    });
    let mut s = failing("settings-retry", Some(gone), None, None);
    s.io = None;
    let mut rt = runtime(s);
    assert!(has_key(&rt, "result-error"));
    assert!(has_text(&rt, "Something went wrong"));
    assert!(has_text(&rt, "No such API route: /api/settings."));
    click(&mut rt, "result-retry");
    assert!(rt.state.settings.retrying);
    assert!(has_text(&rt, "Retrying…"));
    assert!(matches!(
        rt.state.pending.as_slice(),
        [Request::Settings { .. }]
    ));
    let rid = rt.state.settings.rid;
    rt.update(|s, _| {
        s.apply_settings(
            rid,
            Err(LoadError::Transport {
                status: None,
                message: "offline".into(),
            }),
        );
    });
    settle(&mut rt);
    assert!(has_text(&rt, "Request failed"));
    assert!(has_text(&rt, "offline"));
    assert!(has_text(&rt, "Retry"));
}

#[test]
fn a_failed_info_refetch_keeps_the_form() {
    let mut rt = runtime(failing("settings-info", None, None, None));
    click(&mut rt, "settings-add-claude");
    let rid = rt.state.info.rid;
    rt.update(|s, _| {
        s.apply_server_info(rid, Err(LoadError::Decode("x".into())));
    });
    settle(&mut rt);
    assert!(has_key(&rt, "settings-editor"));
    assert_eq!(rt.state.settings.rows[0].len(), 1);

    let mut fresh = runtime(failing(
        "settings-info-first",
        None,
        Some(LoadError::Decode("bad".into())),
        None,
    ));
    assert!(has_key(&fresh, "result-error"));
    assert!(has_text(&fresh, "Unexpected response: bad"));
    click(&mut fresh, "result-retry");
    assert!(fresh.state.info.retrying);
}

#[test]
fn a_reply_for_an_older_save_changes_nothing() {
    let mut s = state(&home::temp_home("settings-rid"));
    s.save_settings(true);
    let old = s.settings.save_rid;
    s.settings.busy = false;
    s.save_settings(true);
    assert!(!s.apply_save(old, Err(LoadError::Decode(String::new())), 0.0));
    assert!(s.settings.busy);
}

#[test]
fn retry_is_disabled_while_the_refetch_runs() {
    let gone = LoadError::Api(ApiError::NotFoundError {
        path: "/api/settings".into(),
    });
    let mut s = failing("settings-retry-busy", Some(gone), None, None);
    s.io = None;
    let mut rt = runtime(s);
    click(&mut rt, "result-retry");
    let rid = rt.state.settings.rid;
    click(&mut rt, "result-retry");
    assert_eq!(
        rt.state.settings.rid, rid,
        "a disabled Retry queues nothing"
    );
    assert_eq!(rt.state.pending.len(), 1);
}

#[test]
fn a_loaded_page_has_no_error_and_no_retry() {
    let rt = fixed_page(false);
    assert!(has_key(&rt, "settings-editor"));
    assert!(!has_key(&rt, "result-error"));
    assert!(!has_key(&rt, "result-retry"));
    assert!(!has_key(&rt, "result-pending"));
}
