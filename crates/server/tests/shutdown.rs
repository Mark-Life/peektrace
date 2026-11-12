//! An open `/api/events` stream must not hold up shutdown.

mod common;

use std::io::{Read as _, Write as _};
use std::sync::Arc;
use std::time::Duration;

use peektrace_core::header_cache::HeaderCache;
use peektrace_core::js::time::Utc;
use peektrace_protocol::WatchVersions;
use peektrace_server::guard::Allowlist;
use peektrace_server::{AppState, Assets, Guards};
use tokio::sync::watch;

#[test]
fn shutdown_ends_open_event_streams() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (_versions, rx) = watch::channel(WatchVersions::default());
    let (stop, stop_rx) = watch::channel(false);
    let state = AppState {
        roots: common::fixture_roots(),
        settings_path: common::temp_dir("shutdown")
            .join("settings.json")
            .to_string_lossy()
            .into_owned(),
        user_home: "/home/test".to_owned(),
        read_only: true,
        zone: Arc::new(Utc),
        versions: rx,
        stop: stop_rx,
        headers: HeaderCache::default(),
        stats_cache_root: "/nonexistent-stats".to_owned(),
        tmp_dir: "/tmp".to_owned(),
        now: peektrace_server::system_clock(),
        stats_lock: Arc::default(),
    };
    let guards = Guards {
        allow: Allowlist::new("127.0.0.1", port),
        token: None,
        port,
    };
    let app = peektrace_server::app(
        Arc::new(state),
        Arc::new(Assets::Embedded),
        Arc::new(guards),
    );
    let (fire, fired) = tokio::sync::oneshot::channel::<()>();
    let server = runtime.spawn(async move {
        let shutdown = async move {
            let _ = fired.await;
            stop.send_replace(true);
        };
        peektrace_server::serve(listener, app, shutdown).await
    });

    let mut conn = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    conn.set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    write!(
        conn,
        "GET /api/events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
    )
    .expect("request");
    let mut first = [0_u8; 512];
    let n = conn.read(&mut first).expect("first event");
    let head = String::from_utf8_lossy(&first[..n]);
    assert!(head.contains("text/event-stream"), "{head}");

    fire.send(()).expect("fire");
    let done =
        runtime.block_on(async { tokio::time::timeout(Duration::from_secs(3), server).await });
    assert!(
        matches!(done, Ok(Ok(Ok(())))),
        "serve still waits on the open stream"
    );
    let mut rest = Vec::new();
    let _ = conn.read_to_end(&mut rest);
}
