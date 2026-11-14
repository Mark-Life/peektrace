//! Runtime helpers for the memory tests.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use peektrace_ui::source::DataSource;
use peektrace_ui::source::in_process::InProcess;
use peektrace_ui::state::{Io, Route, Ui};
use peektrace_ui::{load_now, views};
use rune::{App, Env, Runtime};

pub use crate::common::home;
use crate::common::memory_home::{self, SEED};

pub const SIZE: (f32, f32) = (1440.0, 900.0);

pub fn now_ms() -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
    peektrace_js::time::parse_date(home::NOW, &peektrace_js::time::Utc).map_or(0.0, |ms| ms as f64)
}

pub fn state(home: &Path) -> Ui {
    let dir = home.to_string_lossy().into_owned();
    let peektrace = format!("{dir}/.peektrace");
    let source: Arc<dyn DataSource> = Arc::new(InProcess::for_home(dir, move |k| {
        (k == "PEEKTRACE_DIR").then(|| peektrace.clone())
    }));
    let handle = App::new(peektrace_ui::spec()).handle();
    let mut s = Ui::new(Some(Io { source, handle }), now_ms(), true);
    s.request_sessions();
    load_now(&mut s);
    s
}

pub fn settle(rt: &mut Runtime<Ui>) {
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
}

pub fn runtime(s: Ui) -> Runtime<Ui> {
    let mut rt = Runtime::new(s, views::view, SIZE, Env::new(true, 1.0));
    settle(&mut rt);
    rt
}

pub fn texts(rt: &Runtime<Ui>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter_map(|e| e.text.or(e.label))
        .collect()
}

pub fn mem_home() -> PathBuf {
    let h = home::temp_home("mem");
    memory_home::build_mem_home(&h);
    memory_home::seed_session(&h);
    h
}

pub fn on_memory(home: &Path) -> Ui {
    let mut s = state(home);
    s.go(Route::Memory);
    load_now(&mut s);
    s
}

pub fn has(rt: &Runtime<Ui>, needle: &str) -> bool {
    texts(rt).iter().any(|t| t.contains(needle))
}

pub fn present(rt: &Runtime<Ui>, key: &str) -> bool {
    rt.find(key).is_some()
}

/// Scroll the section until `key` is on screen.
pub fn show(rt: &mut Runtime<Ui>, key: &str) {
    let k = key.to_owned();
    rt.update(move |_, tx| tx.scroll_into_view(k));
    settle(rt);
}

pub fn click(rt: &mut Runtime<Ui>, key: &str) {
    show(rt, key);
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

/// Run queued loads and writes until none are left, as the app would.
pub fn drain(rt: &mut Runtime<Ui>) {
    for _ in 0..4 {
        rt.update(|s, _| load_now(s));
    }
    settle(rt);
}

pub fn mem_runtime(home: &Path) -> Runtime<Ui> {
    runtime(on_memory(home))
}

pub fn seed_file(home: &Path, name: &str) -> PathBuf {
    home.join(format!(".claude/projects/{SEED}/memory/{name}"))
}

pub fn vault_slugs(s: &Ui) -> Vec<String> {
    let m = &s.memory;
    let Some(all) = m.all() else { return vec![] };
    peektrace_ui::model::memory::visible_vaults(all, &m.scope, &m.query.text, &m.ty)
        .iter()
        .map(|v| v.vault.slug.clone())
        .collect()
}
