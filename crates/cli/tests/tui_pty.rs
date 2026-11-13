//! `peektrace tui` on a pseudo-terminal: closing the terminal ends it and
//! still records its `cli` event.
//! Python's `pty.fork` makes the terminal the child's controlling one, as
//! a real terminal window is; skipped when `python3` is missing.
#![cfg(unix)]

use std::net::TcpListener;
use std::process::Command;

use rusqlite::Connection;

const BIN: &str = env!("CARGO_BIN_EXE_peektrace");

/// Start the TUI on a fresh pty, let it draw for 3 s, close the master
/// end, then print `exited` or `alive` after up to 8 s.
const HARNESS: &str = r#"
import os, pty, select, sys, time
pid, fd = pty.fork()
if pid == 0:
    os.execv(sys.argv[1], [sys.argv[1], "tui", "--no-open", "--port", sys.argv[2]])
end = time.time() + 3
while time.time() < end:
    if select.select([fd], [], [], 0.1)[0]:
        try:
            os.read(fd, 65536)
        except OSError:
            break
os.close(fd)
end = time.time() + 8
while time.time() < end:
    if os.waitpid(pid, os.WNOHANG)[0]:
        print("exited")
        sys.exit(0)
    time.sleep(0.05)
os.kill(pid, 9)
os.waitpid(pid, 0)
print("alive")
"#;

fn fixtures(sub: &str) -> String {
    format!(
        "{}/../../packages/core/test/fixtures/{sub}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("free port")
}

#[test]
fn closing_the_terminal_ends_the_tui() {
    if Command::new("python3").arg("--version").output().is_err() {
        eprintln!("skipped: no python3");
        return;
    }
    let dir = std::env::temp_dir().join(format!("peektrace-tui-pty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let out = Command::new("python3")
        .args(["-I", "-c", HARNESS, BIN, &free_port().to_string()])
        .env_clear()
        .env("HOME", &dir)
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("TERM", "xterm-256color")
        .env("PEEKTRACE_DIR", dir.join(".peektrace"))
        .env("PEEKTRACE_TELEMETRY_EXIT_WAIT_MS", "10000")
        .env("PEEKTRACE_CLAUDE_PROJECTS", fixtures("sessions/projects"))
        .env("TZ", "UTC")
        .current_dir(&dir)
        .output()
        .expect("python3");
    let cli_rows = Connection::open(dir.join(".peektrace/telemetry.db")).and_then(|c| {
        c.query_row(
            "SELECT count(*) FROM events WHERE kind = 'cli' AND name = 'cli'",
            [],
            |r| r.get::<_, i64>(0),
        )
    });
    let _ = std::fs::remove_dir_all(&dir);
    let said = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        said.trim(),
        "exited",
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(cli_rows.ok(), Some(1));
}
