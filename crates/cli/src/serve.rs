//! `peektrace serve`: bind, watch every agent root, and serve the API and
//! the web UI until SIGINT or SIGTERM.

use std::process::{Command, Stdio};
use std::sync::Arc;

use peektrace_core::header_cache::HeaderCache;
use peektrace_core::js::os::tmpdir;
use peektrace_core::stats::cache::cache_root;
use peektrace_core::watch::Watcher;
use peektrace_protocol::WatchVersions;
use peektrace_server::auth::{self, TOKEN_ENV};
use peektrace_server::bind::bind_free_port;
use peektrace_server::guard::Allowlist;
use peektrace_server::{AppState, Assets, Guards, system_clock};
use tokio::sync::watch;

use crate::args::ServeArgs;
use crate::ls::{Context, UserError};
use crate::single::agent_roots;
use crate::zone::SystemZone;

const MIN_PORT: i64 = 1;
const MAX_PORT: i64 = 65_535;

/// What `serve` prints before it starts answering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    pub port: u16,
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
    pub open_url: String,
}

/// The startup lines for a bound server.
pub fn banner(host: &str, port: u16, read_only: bool, token: Option<&str>, ui: &str) -> Banner {
    let display = if matches!(host, "0.0.0.0" | "::") {
        "127.0.0.1"
    } else {
        host
    };
    let url = if display.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("http://[{display}]:{port}")
    } else {
        format!("http://{display}:{port}")
    };
    let open_url = token.map_or_else(|| url.clone(), |t| auth::token_url(&url, t));
    let mut stdout = vec![
        format!("Peektrace serving on {url}"),
        format!("  API:    {url}/api"),
        format!("  UI:     {url}/  (from {ui})"),
        format!(
            "  Mode:   {}",
            if read_only { "read-only" } else { "read-write" }
        ),
        "  Watch:  on (filesystem-driven refresh via /api/events)".to_owned(),
    ];
    let mut stderr = Vec::new();
    if token.is_some() {
        stderr.push(format!(
            "  WARNING: bound to {host} (not loopback). Token required; read-only unless --allow-write; plain HTTP, so prefer ssh -L."
        ));
        stdout.push(format!("  Open:   {open_url}"));
        stdout.push(format!(
            "  Remote: from another machine use http://<this-host>:{port}/?token=..."
        ));
    }
    Banner {
        port,
        stdout,
        stderr,
        open_url,
    }
}

/// Put the child outside the server's process group, so Ctrl-C on `serve`
/// does not reach a browser that `xdg-open` keeps in the foreground.
#[cfg(unix)]
fn detached(cmd: &mut Command) -> &mut Command {
    use std::os::unix::process::CommandExt as _;
    cmd.process_group(0)
}

#[cfg(windows)]
fn detached(cmd: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt as _;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
}

#[cfg(not(any(unix, windows)))]
fn detached(cmd: &mut Command) -> &mut Command {
    cmd
}

fn open_browser(url: &str) {
    let (cmd, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else if cfg!(windows) {
        ("cmd", vec!["/c", "start", "", url])
    } else {
        ("xdg-open", vec![url])
    };
    let spawned = detached(
        Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .spawn();
    if let Ok(mut child) = spawned {
        std::thread::spawn(move || child.wait());
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = term => {},
    }
}

/// Everything `serve` needs before it runs.
pub struct Prepared {
    pub banner: Banner,
    pub warnings: Vec<String>,
    pub open: bool,
    listener: std::net::TcpListener,
    state: AppState,
    assets: Assets,
    guards: Guards,
    roots: peektrace_core::AgentRoots,
    tx: watch::Sender<WatchVersions>,
    stop: watch::Sender<bool>,
}

/// Validate flags, pick the token, bind, and build the server state.
pub fn prepare(args: &ServeArgs, read_only: bool, ctx: &Context) -> Result<Prepared, UserError> {
    if !(MIN_PORT..=MAX_PORT).contains(&args.port) {
        return Err(UserError::Invalid(format!(
            "Invalid --port {}: must be an integer between {MIN_PORT} and {MAX_PORT}.",
            args.port
        )));
    }
    let posture = auth::posture(&args.host, read_only, args.allow_write);
    let token = posture
        .auth
        .then(|| auth::resolve_token(std::env::var(TOKEN_ENV).ok().as_deref()))
        .transpose()
        .map_err(UserError::Invalid)?;
    let start = u16::try_from(args.port).unwrap_or(u16::MAX);
    let (listener, port) =
        bind_free_port(&args.host, start).map_err(|e| UserError::Invalid(e.to_string()))?;
    let (roots, warnings) = agent_roots(ctx);
    let cwd = std::env::current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    let client_dir = std::env::var("PEEKTRACE_CLIENT_DIR").ok();
    let (assets, ui) = Assets::resolve(client_dir.as_deref(), &cwd);
    let (tx, rx) = watch::channel(WatchVersions::default());
    let (stop, stop_rx) = watch::channel(false);
    let state = AppState {
        roots: roots.clone(),
        settings_path: ctx.roots_env.settings_path(),
        user_home: ctx.roots_env.home.clone(),
        read_only: posture.read_only,
        zone: Arc::new(SystemZone::default()),
        versions: rx,
        stop: stop_rx,
        headers: HeaderCache::default(),
        stats_cache_root: cache_root(&ctx.roots_env),
        tmp_dir: tmpdir(),
        now: system_clock(),
        stats_lock: Arc::default(),
    };
    let guards = Guards {
        allow: Allowlist::new(&args.host, port),
        token: token.clone(),
        port,
    };
    Ok(Prepared {
        banner: banner(&args.host, port, posture.read_only, token.as_deref(), &ui),
        warnings,
        open: !args.no_open,
        listener,
        state,
        assets,
        guards,
        roots,
        tx,
        stop,
    })
}

/// Serve until a stop signal. `ready` runs once the server can accept.
pub fn run(p: Prepared, ready: impl FnOnce()) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let tx = p.tx;
    let _watcher = Watcher::start(&p.roots, move |v| {
        tx.send_replace(WatchVersions {
            memory: v.memory,
            sessions: v.sessions,
        });
    });
    let app = peektrace_server::app(Arc::new(p.state), Arc::new(p.assets), Arc::new(p.guards));
    runtime.block_on(async move {
        p.listener.set_nonblocking(true)?;
        let listener = tokio::net::TcpListener::from_std(p.listener)?;
        ready();
        if p.open {
            open_browser(&p.banner.open_url);
        }
        let stop = p.stop;
        let shutdown = async move {
            shutdown_signal().await;
            stop.send_replace(true);
        };
        peektrace_server::serve(listener, app, shutdown).await
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_banner() {
        let b = banner("127.0.0.1", 4321, false, None, "embedded");
        assert_eq!(b.stdout[0], "Peektrace serving on http://127.0.0.1:4321");
        assert_eq!(
            b.stdout[2],
            "  UI:     http://127.0.0.1:4321/  (from embedded)"
        );
        assert_eq!(b.stdout.len(), 5);
        assert_eq!(b.stderr, Vec::<String>::new());
    }

    #[test]
    fn authed_banner() {
        let b = banner("0.0.0.0", 4620, true, Some("a b"), "/ui");
        assert_eq!(b.open_url, "http://127.0.0.1:4620/?token=a%20b");
        assert_eq!(b.stdout[3], "  Mode:   read-only");
        assert_eq!(b.stdout[5], "  Open:   http://127.0.0.1:4620/?token=a%20b");
        assert_eq!(b.stderr.len(), 1);
        let b = banner("::1", 1, false, None, "x");
        assert_eq!(b.stdout[0], "Peektrace serving on http://[::1]:1");
        assert_eq!(b.open_url, "http://[::1]:1");
        let b = banner("fd7a::1", 2, true, Some("t"), "x");
        assert_eq!(b.open_url, "http://[fd7a::1]:2/?token=t");
    }

    #[cfg(unix)]
    #[test]
    fn browser_runs_in_its_own_process_group() {
        let out = detached(Command::new("sh").args(["-c", "echo $$; ps -o pgid= -p $$"]))
            .output()
            .expect("sh");
        let text = String::from_utf8_lossy(&out.stdout);
        let ids: Vec<&str> = text.split_whitespace().collect();
        assert_eq!(ids.len(), 2, "{text}");
        assert_eq!(ids[0], ids[1], "child leads its own group");
    }
}
