//! The web server run beside the TUI: on a thread of its own, quietly, fed
//! by the same watcher.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use peektrace_core::watch::Watcher;
use peektrace_protocol::WatchVersions;

use super::{GRACE, Prepared, until_forced, watch_warnings};

/// SIGTERM or SIGHUP: a closed terminal ends the TUI. Ctrl-C reaches the
/// TUI as a key, since raw mode turns signals off.
async fn tui_signal() {
    #[cfg(unix)]
    tokio::select! {
        () = crate::signals::unix_signal(tokio::signal::unix::SignalKind::terminate()) => {},
        () = crate::signals::unix_signal(tokio::signal::unix::SignalKind::hangup()) => {},
    }
    #[cfg(not(unix))]
    std::future::pending::<()>().await;
}

/// What `tui` needs from a server it runs beside itself.
pub struct Sibling {
    pub roots: peektrace_core::AgentRoots,
    pub settings_path: String,
    pub read_only: bool,
    pub stats_cache_root: String,
    pub stats_lock: Arc<tokio::sync::Mutex<()>>,
    /// Watch versions, as the server's clients see them.
    pub versions: mpsc::Receiver<peektrace_core::watch::WatchVersions>,
    /// One line per root the watcher could not cover.
    pub warnings: Vec<String>,
    /// Set by SIGTERM or SIGHUP; the TUI quits when it turns true.
    pub stopped: Arc<AtomicBool>,
    /// Gets one message when `stopped` turns true.
    pub signalled: mpsc::Receiver<()>,
    pub handle: SiblingHandle,
}

/// Stops the sibling server.
pub struct SiblingHandle {
    stop: tokio::sync::oneshot::Sender<()>,
    thread: std::thread::JoinHandle<()>,
}

impl SiblingHandle {
    /// Stop serving: in-flight requests get the grace period, a running
    /// stats pass is abandoned.
    pub fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.thread.join();
    }
}

/// Serve on a thread of its own, quietly, so the caller keeps the main
/// thread. The watcher feeds the server and the returned receiver.
pub fn spawn(p: Prepared) -> anyhow::Result<Sibling> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let (tui_tx, versions) = mpsc::channel();
    let tx = p.tx;
    let watcher = Watcher::start(&p.roots, move |v| {
        tx.send_replace(WatchVersions {
            memory: v.memory,
            sessions: v.sessions,
        });
        let _ = tui_tx.send(v);
    });
    let warnings = watch_warnings(watcher.problems());
    let stopped = Arc::new(AtomicBool::new(false));
    let (signal_tx, signalled) = mpsc::channel();
    let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let state = p.state;
    let roots = state.roots.clone();
    let settings_path = state.settings_path.clone();
    let read_only = state.read_only;
    let stats_cache_root = state.stats_cache_root.clone();
    let stats_lock = Arc::clone(&state.stats_lock);
    let app = peektrace_server::app(Arc::new(state), Arc::new(p.assets), Arc::new(p.guards));
    p.listener.set_nonblocking(true)?;
    let listener = p.listener;
    let flag = Arc::clone(&stopped);
    let server_stop = p.stop;
    let thread = std::thread::spawn(move || {
        let _watcher = watcher;
        runtime.block_on(async move {
            let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                return;
            };
            tokio::spawn(async move {
                tui_signal().await;
                flag.store(true, Ordering::Relaxed);
                let _ = signal_tx.send(());
            });
            let (done, stopping) = tokio::sync::oneshot::channel::<()>();
            let shutdown = async move {
                let _ = stop_rx.await;
                server_stop.send_replace(true);
                let _ = done.send(());
            };
            let stopping = async {
                if stopping.await.is_err() {
                    std::future::pending::<()>().await;
                }
            };
            let served = peektrace_server::serve(listener, app, shutdown);
            let _ = until_forced(served, stopping, std::future::pending, GRACE).await;
        });
        runtime.shutdown_background();
    });
    Ok(Sibling {
        roots,
        settings_path,
        read_only,
        stats_cache_root,
        stats_lock,
        versions,
        warnings,
        stopped,
        signalled,
        handle: SiblingHandle { stop, thread },
    })
}
