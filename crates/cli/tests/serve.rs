//! `peektrace serve` as a process: flags, token, posture, ports and the
//! `--remote` client (`serve-auth-e2e.test.ts`, `find-free-port.test.ts`).

use std::io::{BufRead as _, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
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
    port: u16,
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

fn serve(name: &str, args: &[&str], env: &[(&str, &str)]) -> Served {
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
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(p) = line.strip_prefix("PEEKTRACE_READY:") {
                let _ = tx.send(p.parse::<u16>().unwrap_or(0));
            }
        }
    });
    let port = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("ready line");
    Served { child, port }
}

fn put_settings(port: u16, bearer: Option<&str>) -> u16 {
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent();
    let mut req = agent.put(&format!("http://127.0.0.1:{port}/api/settings"));
    if let Some(t) = bearer {
        req = req.header("Authorization", &format!("Bearer {t}"));
    }
    req.send(r#"{"settings":{}}"#)
        .expect("put")
        .status()
        .as_u16()
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
