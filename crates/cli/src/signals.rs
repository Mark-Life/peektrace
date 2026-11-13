//! Stop signals, and the exit that still records telemetry.

use std::future::{Future, pending};

/// Resolves when `kind` arrives. The handler is in place once this
/// returns; one that fails to register never fires. Needs a runtime context.
#[cfg(unix)]
pub fn unix_signal(kind: tokio::signal::unix::SignalKind) -> impl Future<Output = ()> {
    let registered = tokio::signal::unix::signal(kind).ok();
    async move {
        match registered {
            Some(mut s) => {
                s.recv().await;
            }
            None => pending::<()>().await,
        }
    }
}

/// Resolves on SIGTERM or SIGINT (Ctrl-C off unix), registered as
/// `unix_signal` is.
pub fn stop_signal() -> impl Future<Output = ()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::SignalKind;
        let term = unix_signal(SignalKind::terminate());
        let int = unix_signal(SignalKind::interrupt());
        async move {
            tokio::select! {
                () = term => {},
                () = int => {},
            }
        }
    }
    #[cfg(not(unix))]
    {
        let registered = tokio::signal::windows::ctrl_c().ok();
        async move {
            match registered {
                Some(mut c) => {
                    c.recv().await;
                }
                None => pending::<()>().await,
            }
        }
    }
}

/// Exit with `code` at once. An exit skips the unwinding that closes root
/// spans, so first emit the open ones and flush the store.
pub fn exit_recorded(code: i32) -> ! {
    peektrace_telemetry::finish_open_roots();
    peektrace_telemetry::flush(peektrace_telemetry::exit_wait());
    std::process::exit(code)
}
