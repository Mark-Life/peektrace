//! The Memory section driven through Rune's `Runtime` on temp HOMEs built
//! by `common::memory_home`. Writes land in those temp dirs only.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::home;
use common::memory_home::{self, SEED};
use peektrace_ui::source::DataSource;
use peektrace_ui::source::in_process::InProcess;
use peektrace_ui::state::{Io, Route, Ui};
use peektrace_ui::{load_now, views};
use rune::{App, Env, Runtime};

const SIZE: (f32, f32) = (1440.0, 900.0);

fn now_ms() -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
    peektrace_js::time::parse_date(home::NOW, &peektrace_js::time::Utc).map_or(0.0, |ms| ms as f64)
}

fn state(home: &Path) -> Ui {
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

fn settle(rt: &mut Runtime<Ui>) {
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
}

fn runtime(s: Ui) -> Runtime<Ui> {
    let mut rt = Runtime::new(s, views::view, SIZE, Env::new(true, 1.0));
    settle(&mut rt);
    rt
}

fn texts(rt: &Runtime<Ui>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter_map(|e| e.text.or(e.label))
        .collect()
}

fn mem_home() -> std::path::PathBuf {
    let h = home::temp_home("mem");
    memory_home::build_mem_home(&h);
    memory_home::seed_session(&h);
    h
}

fn on_memory(home: &Path) -> Ui {
    let mut s = state(home);
    s.go(Route::Memory);
    load_now(&mut s);
    s
}

#[test]
fn dump() {
    let rt = runtime(on_memory(&mem_home()));
    for t in texts(&rt) {
        println!("{t}");
    }
    let _ = SEED;
}
