//! `peektrace serve`: bind, watch every agent root, and serve the API and
//! the web UI until SIGINT or SIGTERM.

use std::future::Future;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use peektrace_core::header_cache::HeaderCache;
use peektrace_core::js::os::tmpdir;
use peektrace_core::stats::cache::cache_root;
use peektrace_core::watch::{WatchProblem, Watcher};
use peektrace_protocol::WatchVersions;
use peektrace_server::auth::{self, TOKEN_ENV};
use peektrace_server::bind::bind_free_port;
use peektrace_server::guard::Allowlist;
use peektrace_server::rune_assets::RuneSource;
use peektrace_server::{AppState, Assets, Guards, system_clock};
use tokio::sync::watch;

use crate::args::{ServeArgs, WebUi};
use crate::ls::{Context, UserError};
use crate::signals::stop_signal;
use crate::single::agent_roots;
use peektrace_core::js::system_zone::SystemZone;

mod sibling;

pub use sibling::spawn;

const MIN_PORT: i64 = 1;
const MAX_PORT: i64 = 65_535;
/// How long in-flight requests get to finish after a stop signal. A stats
/// pass cannot be cancelled, so the process does not wait for it.
const GRACE: Duration = Duration::from_secs(2);

const RUNE_DIR_ENV: &str = "PEEKTRACE_RUNE_DIR";
/// WebGPU needs a secure context, which plain HTTP gives only on localhost.
const RUNE_REMOTE: &str = "  WARNING: --ui rune needs WebGPU, which browsers enable only on https or localhost. From another machine, tunnel with ssh -L and open http://127.0.0.1:<port>/.";

/// The Rune source as served assets; a build without the bundle, or a
/// bundle directory without its page, fails instead of serving 503s.
fn rune_assets((src, label): (RuneSource, String)) -> Result<(Assets, String), UserError> {
    match &src {
        RuneSource::Missing => Err(UserError::Invalid(format!(
            "--ui rune: this build does not include the Rune UI. Set {RUNE_DIR_ENV} to a built Rune web app directory, or serve without --ui rune."
        ))),
        RuneSource::Dir(dir) if !dir.join("index.html").is_file() => {
            Err(UserError::Invalid(format!(
                "--ui rune: {RUNE_DIR_ENV} points at {}, which has no index.html. Set it to a built Rune web app directory.",
                dir.display()
            )))
        }
        _ => Ok((Assets::Rune(src), format!("{label}, rune"))),
    }
}

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

/// One stderr line per root whose changes will not reach clients.
pub fn watch_warnings(problems: &[WatchProblem]) -> Vec<String> {
    problems
        .iter()
        .map(|p| format!("  WARNING: {}", p.message()))
        .collect()
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
    let cwd = std::env::current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (assets, ui) = match args.ui {
        WebUi::Inspector => {
            let client_dir = std::env::var("PEEKTRACE_CLIENT_DIR").ok();
            Assets::resolve(client_dir.as_deref(), &cwd)
        }
        WebUi::Rune => {
            let rune_dir = std::env::var(RUNE_DIR_ENV).ok();
            rune_assets(RuneSource::resolve(rune_dir.as_deref(), &cwd))?
        }
    };
    let start = u16::try_from(args.port).unwrap_or(u16::MAX);
    let (listener, port) =
        bind_free_port(&args.host, start).map_err(|e| UserError::Invalid(e.to_string()))?;
    let (roots, warnings) = agent_roots(ctx);
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
        pages: peektrace_server::pages::PagesCache::default(),
    };
    let guards = Guards {
        allow: Allowlist::new(&args.host, port),
        token: token.clone(),
        port,
    };
    let mut banner = banner(&args.host, port, posture.read_only, token.as_deref(), &ui);
    if args.ui == WebUi::Rune && token.is_some() {
        banner.stderr.push(RUNE_REMOTE.to_owned());
    }
    Ok(Prepared {
        banner,
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

/// `served`'s output, or `None` once `grace` passes after `stopping`, or
/// once `force` (made after `stopping`) resolves first.
async fn until_forced<T, F: Future<Output = ()>>(
    served: impl Future<Output = T>,
    stopping: impl Future<Output = ()>,
    force: impl FnOnce() -> F,
    grace: Duration,
) -> Option<T> {
    let forced = async {
        stopping.await;
        tokio::select! {
            () = force() => {},
            () = tokio::time::sleep(grace) => {},
        }
    };
    tokio::select! {
        out = served => Some(out),
        () = forced => None,
    }
}

/// Serve until a stop signal. `ready` runs once the server can accept,
/// with a warning per root the watcher could not cover. Requests still
/// running after a second signal or a short grace period are abandoned.
pub fn run(p: Prepared, ready: impl FnOnce(&[String])) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let tx = p.tx;
    let watcher = Watcher::start(&p.roots, move |v| {
        tx.send_replace(WatchVersions {
            memory: v.memory,
            sessions: v.sessions,
        });
    });
    let app = peektrace_server::app(Arc::new(p.state), Arc::new(p.assets), Arc::new(p.guards));
    let served = runtime.block_on(async move {
        p.listener.set_nonblocking(true)?;
        let listener = tokio::net::TcpListener::from_std(p.listener)?;
        ready(&watch_warnings(watcher.problems()));
        if p.open {
            open_browser(&p.banner.open_url);
        }
        let stop = p.stop;
        let (signalled, stopping) = tokio::sync::oneshot::channel::<()>();
        let shutdown = async move {
            stop_signal().await;
            stop.send_replace(true);
            let _ = signalled.send(());
        };
        let stopping = async {
            if stopping.await.is_err() {
                std::future::pending::<()>().await;
            }
        };
        let served = peektrace_server::serve(listener, app, shutdown);
        until_forced(served, stopping, stop_signal, GRACE)
            .await
            .unwrap_or(Ok(()))
    });
    // Blocking work (a stats pass) never ends early; do not wait for it.
    runtime.shutdown_background();
    served?;
    Ok(())
}

/// Open the web UI at `url` in a browser.
pub fn open(url: &str) {
    open_browser(url);
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

    #[test]
    fn a_build_or_dir_without_the_rune_ui_refuses_to_serve_it() {
        let err = rune_assets((RuneSource::Missing, "not built".into())).unwrap_err();
        assert!(err.to_string().contains(RUNE_DIR_ENV));
        let typo = rune_assets((RuneSource::Dir("/no/such/dir".into()), "x".into())).unwrap_err();
        assert!(
            typo.to_string()
                .contains("/no/such/dir, which has no index.html")
        );
        let dir = std::env::temp_dir().join(format!("rune-dir-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "<p>app</p>").unwrap();
        let (assets, label) = rune_assets((RuneSource::Dir(dir.clone()), "/d".into())).unwrap();
        assert_eq!(assets, Assets::Rune(RuneSource::Dir(dir.clone())));
        assert_eq!(label, "/d, rune");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_watch_limit_names_the_sysctl() {
        let lines = watch_warnings(&[
            WatchProblem::Limit {
                path: "/h/.claude/projects".into(),
            },
            WatchProblem::Unavailable {
                reason: "no backend".to_owned(),
            },
        ]);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("/h/.claude/projects"), "{}", lines[0]);
        assert!(lines[0].contains("fs.inotify.max_user_watches"));
        assert_eq!(lines[1], "  WARNING: live refresh is off (no backend).");
        assert_eq!(watch_warnings(&[]), Vec::<String>::new());
    }

    fn block_on<T>(f: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime")
            .block_on(f)
    }

    #[test]
    fn a_stuck_request_does_not_hold_up_exit() {
        let pending = std::future::pending::<()>;
        let ready = || std::future::ready(());
        let start = std::time::Instant::now();
        let out = block_on(until_forced(
            pending(),
            ready(),
            pending,
            Duration::from_millis(20),
        ));
        assert_eq!(out, None, "grace expires");
        assert!(start.elapsed() < Duration::from_secs(5));
        let out = block_on(until_forced(
            pending(),
            ready(),
            ready,
            Duration::from_secs(3600),
        ));
        assert_eq!(out, None, "second signal forces exit");
        let out = block_on(until_forced(
            std::future::ready(7),
            pending(),
            ready,
            Duration::ZERO,
        ));
        assert_eq!(out, Some(7), "a finished server returns its result");
        let out = block_on(until_forced(
            std::future::ready(7),
            ready(),
            pending,
            Duration::from_secs(3600),
        ));
        assert_eq!(out, Some(7), "requests that finish in time are kept");
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
