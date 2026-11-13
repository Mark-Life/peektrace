//! Telemetry end to end: what lands in `telemetry.db`, the opt-outs, a
//! broken store, and `doctor`. Every run sets `PEEKTRACE_DIR` to a temp dir.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use common::Fixture;
use rusqlite::Connection;

fn run_in(f: &Fixture, dir: &Path, envs: &[(&str, &str)], args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_peektrace"));
    cmd.args(args)
        .env_clear()
        .env("HOME", &f.home)
        .env("PEEKTRACE_DIR", dir)
        .current_dir(&f.home)
        .stdin(Stdio::null());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("run peektrace")
}

fn db(dir: &Path) -> PathBuf {
    dir.join("telemetry.db")
}

fn rows(dir: &Path) -> Vec<String> {
    let conn = Connection::open(db(dir)).expect("open");
    let mut stmt = conn
        .prepare(
            "SELECT kind || ' ' || name || ' ' || outcome || ' ' || data FROM events ORDER BY ts",
        )
        .expect("prepare");
    stmt.query_map([], |r| r.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn every_invocation_records_one_event() {
    let f = Fixture::new();
    let dir = f.home.join("pt");
    for args in [
        &["sessions", "ls"][..],
        &["--help"],
        &[],
        &["-v"],
        &["sessions", "--help"],
        &["sessions", "analyze", "nope"],
        &["sessions", "ls", "--limit", "abc"],
        &["sessions", "ls", "--bogus"],
    ] {
        run_in(&f, &dir, &[], args);
    }
    let rows = rows(&dir);
    assert_eq!(rows.len(), 8, "{rows:#?}");
    let has = |i: usize, s: &str| assert!(rows[i].contains(s), "{} lacks {s}", rows[i]);
    has(
        0,
        r#""attributes":{"argv":"sessions ls","sessions.unreadable":"#,
    );
    has(0, r#""name":"Sessions.list""#);
    has(1, r#""argv":"--help""#);
    has(2, r#""argv":"""#);
    has(3, r#""argv":"--version""#);
    has(5, "cli cli error ");
    has(
        5,
        r#""error":{"tag":"Error","message":"Session \"nope\" not found (searched all agents).","fields":{}}"#,
    );
    has(6, r#""message":"'abc' is not a integer""#);
    has(7, "cli cli error ");
    assert!(rows[0].starts_with("cli cli success "));
}

#[test]
fn statusline_creates_no_store() {
    let f = Fixture::new();
    let dir = f.home.join("pt");
    let out = run_in(&f, &dir, &[], &["statusline"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(!db(&dir).exists());
}

#[test]
fn typed_search_words_never_reach_telemetry() {
    let f = Fixture::new();
    let dir = f.home.join("pt");
    for args in [
        ["zebrasecret", "--limit", "abcsecret"],
        ["zebrasecret", "--agent", "quokkasecret"],
        ["zebrasecret", "--since", "quokkasecret"],
    ] {
        let out = run_in(
            &f,
            &dir,
            &[],
            &[&["sessions", "search"][..], &args].concat(),
        );
        assert_ne!(out.status.code(), Some(0));
    }
    let all = rows(&dir).join("\n");
    assert!(all.contains("sessions search"));
    for secret in ["zebrasecret", "abcsecret", "quokkasecret"] {
        assert!(!all.contains(secret), "{secret} leaked");
    }
}

/// The TS statusline and search tests run with only `HOME` set and read
/// `<HOME>/.peektrace/telemetry.db`; `HOME` here is the fixture's temp dir.
#[test]
fn default_store_lives_under_home() {
    let f = Fixture::new();
    let run_home = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_peektrace"))
            .args(args)
            .env_clear()
            .env("HOME", &f.home)
            .current_dir(&f.home)
            .stdin(Stdio::null())
            .output()
            .expect("run peektrace")
    };
    let dir = f.home.join(".peektrace");
    let out = run_home(&["statusline"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(!db(&dir).exists());
    let out = run_home(&["sessions", "search", "zebrasecret", "--limit", "abcsecret"]);
    assert_ne!(out.status.code(), Some(0));
    let all = rows(&dir).join("\n");
    assert!(all.contains("sessions search"));
    assert!(!all.contains("zebrasecret") && !all.contains("abcsecret"));
}

/// Extra env, args, and whether the run records.
type Case<'a> = (&'a [(&'a str, &'a str)], &'a [&'a str], bool);

#[test]
fn opt_outs() {
    let f = Fixture::new();
    let cases: [Case<'_>; 10] = [
        (&[], &["sessions", "ls"], true),
        (&[], &["--no-telemetry", "sessions", "ls"], false),
        (&[], &["sessions", "ls", "--no-telemetry"], false),
        (
            &[],
            &["--no-telemetry", "--telemetry", "sessions", "ls"],
            false,
        ),
        (&[], &["--telemetry", "sessions", "ls"], true),
        (
            &[("PEEKTRACE_NO_TELEMETRY", "1")],
            &["sessions", "ls"],
            false,
        ),
        (
            &[("PEEKTRACE_NO_TELEMETRY", "")],
            &["sessions", "ls"],
            false,
        ),
        (&[], &["statusline"], false),
        (
            &[],
            &["--remote", "http://127.0.0.1:1", "statusline"],
            false,
        ),
        (&[], &["--log-level", "info", "statusline"], false),
    ];
    for (i, (envs, args, on)) in cases.iter().enumerate() {
        let dir = f.home.join(format!("pt{i}"));
        run_in(&f, &dir, envs, args);
        assert_eq!(db(&dir).exists(), *on, "{args:?} {envs:?}");
        if *on {
            assert_eq!(rows(&dir).len(), 1);
        }
    }
}

/// A broken store changes nothing a command prints or returns.
#[test]
fn broken_stores_never_fail_a_command() {
    let f = Fixture::new();
    let good = run_in(&f, &f.home.join("good"), &[], &["sessions", "ls"]);
    assert_eq!(good.status.code(), Some(0));
    let garbage = f.home.join("garbage");
    fs::create_dir_all(&garbage).unwrap();
    fs::write(db(&garbage), "garbage ".repeat(20)).unwrap();
    let as_dir = f.home.join("as-dir");
    fs::create_dir_all(db(&as_dir)).unwrap();
    let a_file = f.home.join("a-file");
    fs::write(&a_file, "x").unwrap();
    let locked = f.home.join("locked");
    run_in(&f, &locked, &[], &["--version"]);
    let holder = Connection::open(db(&locked)).unwrap();
    holder.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let mut dirs = vec![garbage, as_dir, a_file, locked, PathBuf::new()];
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let ro = f.home.join("ro");
        fs::create_dir_all(&ro).unwrap();
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o555)).unwrap();
        dirs.push(ro);
    }
    for dir in &dirs {
        let started = Instant::now();
        let out = run_in(&f, dir, &[], &["sessions", "ls"]);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{dir:?} too slow"
        );
        assert_eq!(out.status.code(), Some(0), "{dir:?}");
        assert_eq!(text(&out.stdout), text(&good.stdout), "{dir:?}");
        assert_eq!(text(&out.stderr), text(&good.stderr), "{dir:?}");
    }
    assert!(!f.home.join("telemetry.db").exists());
    drop(holder);
}

#[test]
fn parallel_runs_all_land() {
    let f = Fixture::new();
    let dir = f.home.join("pt");
    let children: Vec<_> = (0..8)
        .map(|_| {
            Command::new(env!("CARGO_BIN_EXE_peektrace"))
                .args(["sessions", "ls"])
                .env_clear()
                .env("HOME", &f.home)
                .env("PEEKTRACE_DIR", &dir)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn")
        })
        .collect();
    for mut c in children {
        assert!(c.wait().expect("wait").success());
    }
    assert_eq!(rows(&dir).len(), 8);
}

fn seed(dir: &Path, data: &[(&str, i64, &str)]) {
    create_store(dir);
    let conn = Connection::open(db(dir)).unwrap();
    for (id, ts, json) in data {
        conn.execute(
            "INSERT INTO events VALUES (?1, ?2, 'cli', 'cli', 'success', NULL, 5, 0, ?3)",
            rusqlite::params![id, ts, json],
        )
        .unwrap();
    }
}

/// Create the store the way the binary does.
fn create_store(dir: &Path) {
    let out = Command::new(env!("CARGO_BIN_EXE_peektrace"))
        .args(["--no-telemetry", "doctor", "--out", "/dev/null"])
        .env_clear()
        .env("PEEKTRACE_DIR", dir)
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
}

fn now() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

#[test]
fn doctor_redacts_the_bundle() {
    let f = Fixture::new();
    let dir = f.home.join("pt");
    let secret = format!("sk-ant-{}", "A".repeat(50));
    let event = format!(
        r#"{{"id":"e1","traceId":"t","ts":{},"kind":"cli","name":"cli","appVersion":"0.0.1","platform":"test","durationMs":5,"attributes":{{"argv":"sessions ls","token":"{secret}"}},"spans":[],"outcome":"success"}}"#,
        now()
    );
    seed(&dir, &[("e1", now(), &event)]);
    let out = run_in(&f, &dir, &[("PEEKTRACE_NO_TELEMETRY", "1")], &["doctor"]);
    let path = dir.join("peektrace-report-1.json");
    assert_eq!(
        text(&out.stdout),
        format!(
            "Wrote 1 events → {}\nEmail this file to 108@mark-life.com\n",
            path.display()
        )
    );
    let raw = fs::read_to_string(&path).unwrap();
    assert!(!raw.contains(&secret));
    assert!(!raw.ends_with('\n'));
    let bundle: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(bundle["schema"], "peektrace-report/v1");
    assert_eq!(bundle["count"], 1);
    assert_eq!(
        bundle["events"][0]["attributes"]["token"],
        "[REDACTED:anthropic-key]"
    );
    assert_eq!(bundle["events"][0]["attributes"]["argv"], "sessions ls");
}

#[test]
fn doctor_selection_and_unreadable_line() {
    let f = Fixture::new();
    let dir = f.home.join("pt");
    let ev = |id: &str, ts: i64, attrs: &str| {
        format!(
            r#"{{"id":"{id}","traceId":"t","ts":{ts},"kind":"rpc","name":"rpc.sessions.listing","appVersion":"v","platform":"p","durationMs":1,"attributes":{attrs},"spans":[],"outcome":"success"}}"#
        )
    };
    let at = now();
    let (older, newer, newest) = (
        ev("a", at - 3, r#"{"sessions.unreadable":5}"#),
        ev("b", at - 2, r#"{"sessions.unreadable":2}"#),
        ev("c", at - 1, "{}"),
    );
    seed(
        &dir,
        &[
            ("a", at - 3, &older),
            ("b", at - 2, &newer),
            ("c", at - 1, &newest),
            ("bad", at, "{\"kind\":\"tui\"}"),
        ],
    );
    let quiet = [("PEEKTRACE_NO_TELEMETRY", "1")];
    let out = run_in(&f, &dir, &quiet, &["doctor", "--out", "x.json"]);
    assert_eq!(
        text(&out.stdout),
        "Wrote 3 events → x.json\n2 sessions could not be read (last sessions list)\nEmail this file to 108@mark-life.com\n"
    );
    let out = run_in(
        &f,
        &dir,
        &quiet,
        &["doctor", "--last", "1", "--out", "y.json"],
    );
    assert!(text(&out.stdout).starts_with("Wrote 0 events → y.json\nEmail"));
    let out = run_in(
        &f,
        &dir,
        &quiet,
        &["doctor", "--last", "-1", "--interesting-only"],
    );
    assert!(text(&out.stdout).starts_with("Wrote 0 events → "));
    let out = run_in(&f, &dir, &quiet, &["doctor", "--last", "abc"]);
    assert_eq!(
        (out.status.code(), text(&out.stderr)),
        (Some(1), "'abc' is not a integer\n\n".to_owned())
    );
    let out = run_in(&f, &dir, &quiet, &["doctor", "--out", "missing/x.json"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        "NotFound: FileSystem.writeFile (missing/x.json): ENOENT: no such file or directory, open 'missing/x.json'\n"
    );
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn doctor_opens_the_store_even_opted_out() {
    let f = Fixture::new();
    let dir = f.home.join("fresh");
    let out = run_in(&f, &dir, &[], &["--no-telemetry", "doctor"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(db(&dir).exists());
    assert!(dir.join("peektrace-report-0.json").exists());
    let out = run_in(&f, Path::new(""), &[], &["doctor"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).starts_with("Could not open the telemetry store: "));
}

#[test]
fn a_panic_lands_as_a_defect() {
    let f = Fixture::new();
    for (point, message) in [("main", "test panic"), ("worker", "worker panic")] {
        let dir = f.home.join(point);
        let out = run_in(&f, &dir, &[("PEEKTRACE_TEST_PANIC", point)], &["-v"]);
        assert_eq!(out.status.code(), Some(101), "{}", text(&out.stderr));
        assert!(text(&out.stderr).contains(message));
        let rows = rows(&dir);
        assert_eq!(rows.len(), 1, "{rows:#?}");
        assert!(rows[0].starts_with("cli cli defect "), "{}", rows[0]);
        assert!(
            rows[0].contains(&format!(r#""error":{{"message":"{message}"}}"#)),
            "{}",
            rows[0]
        );
    }
}

/// Both stop signals exit 0, as TS's runtime does on an interrupt.
#[cfg(unix)]
#[test]
fn mcp_stopped_by_a_signal_still_records() {
    for signal in ["-TERM", "-INT"] {
        mcp_stopped_by(signal);
    }
}

#[cfg(unix)]
fn mcp_stopped_by(signal: &str) {
    use std::io::{BufRead as _, BufReader, Write as _};
    let f = Fixture::new();
    let dir = f.home.join("pt");
    let mut child = Command::new(env!("CARGO_BIN_EXE_peektrace"))
        .arg("mcp")
        .env_clear()
        .env("HOME", &f.home)
        .env("PEEKTRACE_DIR", &dir)
        .current_dir(&f.home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mcp");
    let mut stdin = child.stdin.take().expect("stdin");
    let call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"sessions_list","arguments":{}}}"#;
    writeln!(stdin, "{call}").expect("write");
    let mut reply = String::new();
    BufReader::new(child.stdout.take().expect("stdout"))
        .read_line(&mut reply)
        .expect("reply");
    assert!(reply.contains(r#""id":1"#), "{reply}");
    let killed = Command::new("kill")
        .args([signal, &child.id().to_string()])
        .status()
        .expect("kill");
    assert!(killed.success());
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(s) = child.try_wait().expect("wait") {
            break s;
        }
        assert!(Instant::now() < deadline, "mcp ignored {signal}");
        std::thread::sleep(Duration::from_millis(20));
    };
    drop(stdin);
    assert_eq!(status.code(), Some(0), "{signal}");
    let rows = rows(&dir);
    assert_eq!(rows.len(), 2, "{signal} {rows:#?}");
    assert!(
        rows.iter()
            .any(|r| r.starts_with("rpc rpc.sessions.listing success ")),
        "{rows:#?}"
    );
    assert!(
        rows.iter()
            .any(|r| r.starts_with("cli cli success ") && r.contains(r#""argv":"mcp""#)),
        "{rows:#?}"
    );
}

#[test]
fn stats_keeps_one_load_span_per_transcript() {
    let f = Fixture::new();
    let dir = f.home.join("pt");
    let out = run_in(&f, &dir, &[], &["stats"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let rows = rows(&dir);
    assert_eq!(rows.len(), 1, "{rows:#?}");
    let transcripts: usize = rows[0]
        .split(r#""name":"Stats.foldCorpus","durationMs":"#)
        .nth(1)
        .and_then(|s| s.split(r#""transcripts":"#).nth(1))
        .and_then(|s| s.split('}').next())
        .and_then(|n| n.parse().ok())
        .expect("foldCorpus span");
    assert!(transcripts > 0);
    assert_eq!(
        rows[0]
            .matches(r#""name":"AgentRegistry.loadTranscript""#)
            .count(),
        transcripts,
        "{}",
        rows[0]
    );
}
