//! `peektrace serve` as a process: flags, token, posture, ports and the
//! `--remote` client (`serve-auth-e2e.test.ts`, `find-free-port.test.ts`).

use std::io::{BufRead as _, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError, mpsc};
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_peektrace");
const TOKEN: &str = "abcdefghijklmnopqrstuvwxyz0123456789";

fn fixtures(sub: &str) -> String {
    format!(
        "{}/../../packages/core/test/fixtures/{sub}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn base_command(dir: &str) -> Command {
    let mut c = Command::new(BIN);
    c.env_clear()
        .env("HOME", dir)
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("PEEKTRACE_DIR", format!("{dir}/.peektrace"))
        .env("PEEKTRACE_CLAUDE_PROJECTS", fixtures("sessions/projects"))
        .env("PEEKTRACE_CODEX_SESSIONS", fixtures("sessions-codex"))
        .env("PEEKTRACE_CLIENT", "desktop")
        .env("TZ", "UTC");
    // Windows sockets need SystemRoot.
    if let Ok(root) = std::env::var("SystemRoot") {
        c.env("SystemRoot", root);
    }
    c
}

fn temp(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("peektrace-serve-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("mkdir");
    d.to_string_lossy().into_owned()
}

/// A running server; killed on drop.
struct Served {
    child: Child,
    _serial: MutexGuard<'static, ()>,
    port: u16,
    /// Stdout up to the ready line.
    lines: Vec<String>,
    /// The server's HOME; settings live in `<dir>/.peektrace`.
    dir: String,
}

impl Served {
    /// The token in the printed `Open:` URL.
    fn printed_token(&self) -> Option<String> {
        self.lines
            .iter()
            .find_map(|l| l.trim_start().strip_prefix("Open:"))
            .and_then(|url| url.split("?token=").nth(1))
            .map(str::to_owned)
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("free port")
}

/// One server at a time: macOS lets `0.0.0.0:P` and `127.0.0.1:P` both bind,
/// so two servers whose port scans overlap could answer for each other.
static SERIAL: Mutex<()> = Mutex::new(());

fn serve(name: &str, args: &[&str], env: &[(&str, &str)]) -> Served {
    let serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let dir = temp(name);
    let mut c = base_command(&dir);
    c.arg("serve").arg("--no-open").args(args);
    for (k, v) in env {
        c.env(k, v);
    }
    let mut child = c
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut seen = Vec::new();
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(p) = line.strip_prefix("PEEKTRACE_READY:") {
                let port = p.parse::<u16>().unwrap_or(0);
                let _ = tx.send((port, std::mem::take(&mut seen)));
            } else {
                seen.push(line);
            }
        }
    });
    let (port, lines) = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("ready line");
    Served {
        child,
        _serial: serial,
        port,
        lines,
        dir,
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent()
}

fn put_settings_body(port: u16, bearer: Option<&str>) -> (u16, String) {
    let mut req = agent().put(&format!("http://127.0.0.1:{port}/api/settings"));
    if let Some(t) = bearer {
        req = req.header("Authorization", &format!("Bearer {t}"));
    }
    let mut res = req.send(r#"{"settings":{}}"#).expect("put");
    let body = res.body_mut().read_to_string().unwrap_or_default();
    (res.status().as_u16(), body)
}

fn put_settings(port: u16, bearer: Option<&str>) -> u16 {
    put_settings_body(port, bearer).0
}

fn health(port: u16, bearer: Option<&str>) -> u16 {
    let mut req = agent().get(&format!("http://127.0.0.1:{port}/health"));
    if let Some(t) = bearer {
        req = req.header("Authorization", &format!("Bearer {t}"));
    }
    req.call().expect("health").status().as_u16()
}

fn run(dir: &str, args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
    let mut c = base_command(dir);
    c.args(args);
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c.output().expect("run");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn bad_port_and_short_token_fail_before_binding() {
    let dir = temp("flags");
    let (code, _, err) = run(&dir, &["serve", "--no-open", "--port", "0"], &[]);
    assert_eq!(code, 1);
    assert_eq!(
        err,
        "Invalid --port 0: must be an integer between 1 and 65535.\n"
    );
    let port = free_port().to_string();
    let args = ["serve", "--no-open", "--host", "0.0.0.0", "--port", &port];
    let (code, _, err) = run(&dir, &args, &[("PEEKTRACE_TOKEN", "short")]);
    assert_eq!(code, 1);
    assert_eq!(
        err,
        "PEEKTRACE_TOKEN must be at least 32 characters. Unset it to get a random token.\n"
    );
}

#[test]
fn loopback_ignores_the_token_and_writes() {
    let port = free_port().to_string();
    let s = serve(
        "loopback",
        &["--port", &port],
        &[("PEEKTRACE_TOKEN", "short")],
    );
    assert_eq!(put_settings(s.port, None), 200);
}

#[test]
fn non_loopback_is_read_only_unless_allow_write() {
    let token = [("PEEKTRACE_TOKEN", TOKEN)];
    let port = free_port().to_string();
    let s = serve("ro", &["--host", "0.0.0.0", "--port", &port], &token);
    assert_eq!(put_settings(s.port, None), 401);
    assert_eq!(put_settings(s.port, Some(TOKEN)), 403);
    drop(s);
    let port = free_port().to_string();
    let s = serve(
        "rw",
        &["--host", "0.0.0.0", "--allow-write", "--port", &port],
        &token,
    );
    assert_eq!(put_settings(s.port, Some(TOKEN)), 200);
    drop(s);
    let port = free_port().to_string();
    let args = [
        "--host",
        "0.0.0.0",
        "--allow-write",
        "--read-only",
        "--port",
        &port,
    ];
    let s = serve("both", &args, &token);
    assert_eq!(put_settings(s.port, Some(TOKEN)), 403);
}

#[test]
fn busy_port_moves_to_the_next() {
    let busy = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = busy.local_addr().expect("addr").port();
    let s = serve("busy", &["--port", &port.to_string()], &[]);
    assert!(s.port > port, "{} after {port}", s.port);
}

#[test]
fn remote_with_token_url_env_token_or_none() {
    let port = free_port().to_string();
    let s = serve(
        "remote",
        &["--host", "0.0.0.0", "--port", &port],
        &[("PEEKTRACE_TOKEN", TOKEN)],
    );
    let dir = temp("remote-client");
    let base = format!("http://127.0.0.1:{}", s.port);
    let ls = ["sessions", "ls", "--limit", "0"];
    let (code, _, err) = run(&dir, &[&["--remote", &base][..], &ls].concat(), &[]);
    assert_eq!(code, 1);
    assert_eq!(
        err,
        format!(
            "Remote request to {base} was unauthorized (HTTP 401). Set PEEKTRACE_TOKEN or pass the printed ?token= URL.\n"
        )
    );
    let url = format!("{base}/?token={TOKEN}");
    let (code, out, _) = run(&dir, &[&["--remote", &url][..], &ls].concat(), &[]);
    assert_eq!(code, 0);
    assert!(out.contains("11111111"), "{out}");
    let (code, out, _) = run(
        &dir,
        &[&["--remote", &base][..], &ls].concat(),
        &[("PEEKTRACE_TOKEN", TOKEN)],
    );
    assert_eq!(code, 0);
    let local = run(&dir, &ls, &[]);
    assert_eq!(out, local.1);
}

// serve-auth-e2e.test.ts cases with the TS assertions. `memory.create` is not
// in this build, so the write probe is `PUT /api/settings`.

const LONG_TOKEN: &str = "kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk";

#[test]
fn non_loopback_requires_the_token_and_refuses_writes_by_default() {
    let port = free_port().to_string();
    let s = serve("e2e-default", &["--host", "0.0.0.0", "--port", &port], &[]);
    let token = s.printed_token().expect("Open: line");
    assert_eq!(token.len(), 43);
    assert_eq!(health(s.port, None), 401);
    assert_eq!(health(s.port, Some(&token)), 200);
    let (status, body) = put_settings_body(s.port, Some(&token));
    assert_eq!(status, 403);
    assert!(body.contains("read-only"), "{body}");
    assert!(
        !std::path::Path::new(&s.dir)
            .join(".peektrace/settings.json")
            .exists()
    );
}

#[test]
fn allow_write_permits_a_write() {
    let port = free_port().to_string();
    let args = ["--host", "0.0.0.0", "--allow-write", "--port", &port];
    let s = serve("e2e-allow", &args, &[]);
    let token = s.printed_token().expect("Open: line");
    assert_eq!(put_settings(s.port, Some(&token)), 200);
    assert!(
        std::path::Path::new(&s.dir)
            .join(".peektrace/settings.json")
            .exists()
    );
}

#[test]
fn read_only_wins_over_allow_write() {
    let port = free_port().to_string();
    let args = [
        "--host",
        "0.0.0.0",
        "--allow-write",
        "--read-only",
        "--port",
        &port,
    ];
    let s = serve("e2e-both", &args, &[]);
    let token = s.printed_token().expect("Open: line");
    let (status, body) = put_settings_body(s.port, Some(&token));
    assert_eq!(status, 403);
    assert!(body.contains("read-only"), "{body}");
}

#[test]
fn peektrace_token_supplies_the_token() {
    let port = free_port().to_string();
    let s = serve(
        "e2e-env",
        &["--host", "0.0.0.0", "--port", &port],
        &[("PEEKTRACE_TOKEN", LONG_TOKEN)],
    );
    assert_eq!(s.printed_token().as_deref(), Some(LONG_TOKEN));
}

#[test]
fn a_short_token_fails_before_binding() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let dir = temp("e2e-short");
    let port = free_port();
    let args = [
        "serve",
        "--no-open",
        "--host",
        "0.0.0.0",
        "--port",
        &port.to_string(),
    ];
    let (code, _, err) = run(&dir, &args, &[("PEEKTRACE_TOKEN", "short")]);
    assert_ne!(code, 0);
    assert!(err.contains("PEEKTRACE_TOKEN"), "{err}");
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
}

#[test]
fn loopback_ignores_a_long_token_and_needs_no_credentials() {
    let port = free_port().to_string();
    let s = serve(
        "e2e-loopback",
        &["--port", &port],
        &[("PEEKTRACE_TOKEN", LONG_TOKEN)],
    );
    assert_eq!(s.printed_token(), None);
    assert_eq!(health(s.port, None), 200);
}

#[test]
fn remote_json_ls_with_token_url_env_token_and_the_missing_token_message() {
    let port = free_port().to_string();
    let s = serve("e2e-remote", &["--host", "0.0.0.0", "--port", &port], &[]);
    let token = s.printed_token().expect("Open: line");
    let dir = temp("e2e-remote-client");
    let base = format!("http://127.0.0.1:{}", s.port);
    let ls = ["--json", "sessions", "ls"];
    let url = format!("{base}/?token={token}");
    let (code, _, _) = run(&dir, &[&["--remote", url.as_str()][..], &ls].concat(), &[]);
    assert_eq!(code, 0);
    let (code, _, _) = run(
        &dir,
        &[&["--remote", base.as_str()][..], &ls].concat(),
        &[("PEEKTRACE_TOKEN", token.as_str())],
    );
    assert_eq!(code, 0);
    let (code, _, err) = run(&dir, &[&["--remote", base.as_str()][..], &ls].concat(), &[]);
    assert_eq!(code, 1);
    assert!(err.contains("HTTP 401"), "{err}");
    assert!(err.contains("PEEKTRACE_TOKEN"), "{err}");
    assert!(!err.contains(&token), "{err}");
}
