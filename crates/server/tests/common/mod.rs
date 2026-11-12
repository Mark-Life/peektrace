//! An in-process server on `127.0.0.1:0` over the core fixtures.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use peektrace_core::AgentRoots;
use peektrace_core::header_cache::HeaderCache;
use peektrace_core::js::time::Utc;
use peektrace_core::roots::{RootsEnv, Settings};
use peektrace_protocol::WatchVersions;
use peektrace_server::guard::Allowlist;
use peektrace_server::{AppState, Assets, Guards};

pub const SESSION: &str = "11111111-1111-4111-8111-111111111111";

pub fn fixtures(sub: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/core/test/fixtures");
    p.join(sub).to_string_lossy().into_owned()
}

pub fn temp_dir(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "peektrace-server-{name}-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("mkdir");
    d
}

pub fn fixture_roots() -> AgentRoots {
    let env = RootsEnv {
        home: "/nonexistent-home".to_owned(),
        claude_projects: Some(fixtures("sessions/projects")),
        codex_sessions: Some(fixtures("sessions-codex")),
        pi_sessions: Some("/nonexistent-pi".to_owned()),
        opencode_data: Some("/nonexistent-opencode".to_owned()),
        ..RootsEnv::default()
    };
    AgentRoots::resolve(&env, &Settings::default())
}

pub struct TestServer {
    pub port: u16,
    /// The UI directory the server reads on each request.
    pub ui: PathBuf,
    _runtime: tokio::runtime::Runtime,
    pub settings_path: String,
}

pub struct Options<'a> {
    pub name: &'a str,
    pub read_only: bool,
    pub host: &'a str,
    pub token: Option<&'a str>,
    /// Agent roots; the core fixtures when `None`.
    pub roots: Option<AgentRoots>,
    /// Stats cache dir; a fresh one under the test dir when `None`.
    pub stats_dir: Option<String>,
    /// Pinned clock; the wall clock when `None`.
    pub now: Option<peektrace_server::Clock>,
    /// User home for redaction.
    pub home: Option<String>,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            name: "api",
            read_only: false,
            host: "127.0.0.1",
            token: None,
            roots: None,
            stats_dir: None,
            now: None,
            home: None,
        }
    }
}

pub fn start(o: &Options<'_>) -> TestServer {
    let dir = temp_dir(o.name);
    let ui = dir.join("ui");
    std::fs::create_dir_all(&ui).expect("mkdir ui");
    std::fs::write(ui.join("index.html"), "<!doctype html>").expect("index");
    let settings_path = dir.join("settings.json").to_string_lossy().into_owned();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (tx, rx) = tokio::sync::watch::channel(WatchVersions::default());
    let (stop, stop_rx) = tokio::sync::watch::channel(false);
    let state = AppState {
        roots: o.roots.clone().unwrap_or_else(fixture_roots),
        settings_path: settings_path.clone(),
        user_home: o.home.clone().unwrap_or_else(|| "/home/test".to_owned()),
        read_only: o.read_only,
        zone: Arc::new(Utc),
        versions: rx,
        stop: stop_rx,
        headers: HeaderCache::default(),
        stats_cache_root: o
            .stats_dir
            .clone()
            .unwrap_or_else(|| dir.join("stats-rs").to_string_lossy().into_owned()),
        tmp_dir: dir.to_string_lossy().into_owned(),
        now: o.now.clone().unwrap_or_else(peektrace_server::system_clock),
        stats_lock: Arc::default(),
    };
    let guards = Guards {
        allow: Allowlist::new(o.host, port),
        token: o.token.map(str::to_owned),
        port,
    };
    let app = peektrace_server::app(
        Arc::new(state),
        Arc::new(Assets::Dir(ui.clone())),
        Arc::new(guards),
    );
    runtime.spawn(async move {
        let _keep = (tx, stop);
        let _ = peektrace_server::serve(listener, app, std::future::pending()).await;
    });
    TestServer {
        port,
        ui,
        _runtime: runtime,
        settings_path,
    }
}

pub struct Answer {
    pub status: u16,
    pub body: String,
    pub headers: Vec<(String, String)>,
}

impl Answer {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("json body")
    }
}

impl TestServer {
    pub fn call(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> Answer {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .new_agent();
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        let mut req = ureq::http::Request::builder().method(method).uri(&url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let req = req.body(body.unwrap_or("").to_owned()).expect("request");
        let mut res = agent.run(req).expect("response");
        let headers = res
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_owned()))
            .collect();
        Answer {
            status: res.status().as_u16(),
            body: res.body_mut().read_to_string().unwrap_or_default(),
            headers,
        }
    }

    pub fn get(&self, path: &str) -> Answer {
        self.call("GET", path, &[], None)
    }
}
