//! An open `/api/events` stream must not hold up shutdown, and stays open
//! until then even with no watcher behind it.

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

struct Running {
    runtime: tokio::runtime::Runtime,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
    fire: tokio::sync::oneshot::Sender<()>,
    conn: std::net::TcpStream,
    /// Bytes read up to the first event.
    seen: Vec<u8>,
}

/// A server with one `/api/events` stream open past its first event. The
/// versions sender lives as long as the server unless `watcher` is false.
fn open_events(watcher: bool) -> Running {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (versions, rx) = watch::channel(WatchVersions::default());
    let versions = watcher.then_some(versions);
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
        pages: peektrace_server::pages::PagesCache::default(),
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
        let served = peektrace_server::serve(listener, app, shutdown).await;
        drop(versions);
        served
    });

    let mut conn = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    conn.set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    write!(
        conn,
        "GET /api/events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
    )
    .expect("request");
    let mut seen = Vec::new();
    let mut buf = [0_u8; 512];
    while !String::from_utf8_lossy(&seen).contains("event: versions") {
        let n = conn.read(&mut buf).expect("first event");
        assert!(n > 0, "closed before the first event");
        seen.extend_from_slice(&buf[..n]);
    }
    let head = String::from_utf8_lossy(&seen);
    assert!(head.contains("text/event-stream"), "{head}");

    Running {
        runtime,
        server,
        fire,
        conn,
        seen,
    }
}

fn shut_down(r: Running) {
    let Running {
        runtime,
        server,
        fire,
        mut conn,
        ..
    } = r;
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

#[test]
fn shutdown_ends_open_event_streams() {
    shut_down(open_events(true));
}

#[test]
fn the_stream_outlives_a_missing_watcher() {
    let mut r = open_events(false);
    r.conn
        .set_read_timeout(Some(Duration::from_millis(500)))
        .expect("timeout");
    assert!(
        !r.seen.ends_with(b"0\r\n\r\n"),
        "stream ended with the first event"
    );
    let mut buf = [0_u8; 512];
    match r.conn.read(&mut buf) {
        Ok(0) => panic!("stream closed after the first event"),
        Ok(n) => panic!("unexpected data: {}", String::from_utf8_lossy(&buf[..n])),
        Err(e) => assert!(
            matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ),
            "{e}"
        ),
    }
    shut_down(r);
}
