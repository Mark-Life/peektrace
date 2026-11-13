//! `peektrace upgrade` against a local fake release server, replacing a temp
//! file through `PEEKTRACE_UPGRADE_TARGET` (TS `upgrade.test.ts`).
#![cfg(unix)]
#![allow(clippy::case_sensitive_file_extension_comparisons)]

mod common;

use std::fs;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::{Home, Run};
use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest as _, Sha256};

const PAYLOAD: &[u8] = &[0x70, 0x65, 0x65, 0x6b, 9, 9, 9];
const OLD: &[u8] = &[1, 2, 3];

fn host_asset() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = if std::env::consts::ARCH == "aarch64" {
        "arm64"
    } else {
        "x64"
    };
    format!("peektrace-{os}-{arch}")
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut s, b| {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn gz(bytes: &[u8]) -> Vec<u8> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(bytes).expect("gzip");
    enc.finish().expect("gzip")
}

/// How the fake server answers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Ok,
    WrongSum,
    NoGz,
}

struct Server {
    origin: String,
    log: Arc<Mutex<Vec<String>>>,
}

/// Serve releases, assets and `SHA256SUMS` until the test ends.
fn serve(mode: Mode) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let origin = format!("http://{}", listener.local_addr().expect("addr"));
    let log = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&log);
    let asset = host_asset();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { continue };
            let head: Vec<String> = BufReader::new(conn.try_clone().expect("clone"))
                .lines()
                .map_while(Result::ok)
                .take_while(|l| !l.is_empty())
                .collect();
            let path = head
                .first()
                .and_then(|l| l.split(' ').nth(1))
                .unwrap_or("")
                .to_owned();
            seen.lock().expect("log").push(path.clone());
            let (status, body): (u16, Vec<u8>) = if path.starts_with("/releases") {
                (
                    200,
                    br#"[{"tag_name":"desktop-v3.0.0"},{"tag_name":"cli-v9.9.9"}]"#.to_vec(),
                )
            } else if path.ends_with("/SHA256SUMS") {
                let sum = if mode == Mode::WrongSum {
                    "0".repeat(64)
                } else {
                    hex(PAYLOAD)
                };
                (200, format!("{sum}  {asset}\n").into_bytes())
            } else if path.ends_with(".gz") {
                if mode == Mode::NoGz {
                    (404, b"nf".to_vec())
                } else {
                    (200, gz(PAYLOAD))
                }
            } else if path.ends_with(&format!("/{asset}")) {
                (200, PAYLOAD.to_vec())
            } else {
                (404, b"nf".to_vec())
            };
            let res = format!(
                "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = conn.write_all(res.as_bytes());
            let _ = conn.write_all(&body);
        }
    });
    Server { origin, log }
}

impl Server {
    fn requests(&self) -> Vec<String> {
        self.log.lock().expect("log").clone()
    }
}

struct Setup {
    home: Home,
    server: Server,
    target: PathBuf,
}

fn setup(mode: Mode) -> Setup {
    let home = Home::new("upgrade");
    let target = home.dir().join("bin/peektrace");
    fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
    fs::write(&target, OLD).expect("write");
    Setup {
        home,
        server: serve(mode),
        target,
    }
}

impl Setup {
    fn run(&self, args: &[&str]) -> Run {
        let o = self.server.origin.as_str();
        let t = self.target.to_string_lossy();
        self.home.run_stdin(
            args,
            b"",
            &[
                ("PEEKTRACE_GITHUB_API", o),
                ("PEEKTRACE_BASE_URL", o),
                ("PEEKTRACE_UPGRADE_TARGET", &t),
            ],
        )
    }

    fn dir_listing(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.target.parent().expect("parent"))
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

#[test]
fn check_reports_an_available_update_and_writes_nothing() {
    let s = setup(Mode::Ok);
    let r = s.run(&["upgrade", "--check"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(
        r.out,
        "A newer version (cli-v9.9.9) is available — run 'peektrace upgrade'\n"
    );
    assert_eq!(fs::read(&s.target).ok().as_deref(), Some(OLD));
    assert_eq!(s.server.requests(), ["/releases?per_page=100"]);
}

#[test]
fn checksum_mismatch_aborts_and_leaves_the_target_unchanged() {
    let s = setup(Mode::WrongSum);
    let r = s.run(&["upgrade"]);
    assert_eq!(r.code, 1);
    assert_eq!(
        r.err,
        format!(
            "Checksum mismatch for {} (expected {}, got {}). Aborting without installing.\n",
            host_asset(),
            "0".repeat(64),
            hex(PAYLOAD)
        )
    );
    assert_eq!(fs::read(&s.target).ok().as_deref(), Some(OLD));
    assert_eq!(s.dir_listing(), ["peektrace"]);
}

#[test]
fn a_verified_upgrade_replaces_the_target() {
    let s = setup(Mode::Ok);
    let r = s.run(&["upgrade"]);
    let asset = host_asset();
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(
        r.out,
        format!(
            "Fetching {asset} (cli-v9.9.9)...\nUpgraded to cli-v9.9.9 (downloaded 7 B). It takes effect on the next launch.\n"
        )
    );
    assert_eq!(fs::read(&s.target).ok().as_deref(), Some(PAYLOAD));
    let mode = fs::metadata(&s.target).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode, 0o755);
    assert_eq!(s.dir_listing(), ["peektrace"]);
}

#[test]
fn prefers_the_gzipped_asset() {
    let s = setup(Mode::Ok);
    let r = s.run(&["upgrade", "--version", "9.9.9"]);
    assert_eq!(r.code, 0, "{}", r.err);
    let asset = host_asset();
    assert_eq!(
        s.server.requests(),
        [
            format!("/cli-v9.9.9/{asset}.gz"),
            "/cli-v9.9.9/SHA256SUMS".to_owned()
        ]
    );
    assert_eq!(fs::read(&s.target).ok().as_deref(), Some(PAYLOAD));
}

#[test]
fn falls_back_to_the_raw_asset_when_there_is_no_gz() {
    let s = setup(Mode::NoGz);
    let r = s.run(&["upgrade", "--version", "cli-v9.9.9"]);
    assert_eq!(r.code, 0, "{}", r.err);
    let asset = host_asset();
    assert_eq!(
        s.server.requests(),
        [
            format!("/cli-v9.9.9/{asset}.gz"),
            format!("/cli-v9.9.9/{asset}"),
            "/cli-v9.9.9/SHA256SUMS".to_owned()
        ]
    );
    assert_eq!(fs::read(&s.target).ok().as_deref(), Some(PAYLOAD));
}

#[test]
fn version_flag_first_pins_instead_of_printing_the_version() {
    let s = setup(Mode::Ok);
    let r = s.run(&["upgrade", "--version", "1.2.3", "--check"]);
    assert_eq!(
        r.out,
        "A newer version (cli-v1.2.3) is available — run 'peektrace upgrade'\n"
    );
    assert_eq!(s.server.requests(), Vec::<String>::new());
}

#[test]
fn same_version_is_already_latest() {
    let s = setup(Mode::Ok);
    let r = s.run(&["upgrade", "--check", "--version", "cli-v0.0.0"]);
    assert_eq!(r.out, "peektrace is up to date (cli-v0.0.0-dev).\n");
}

/// TS "skips the delta path outside a compiled binary": Rust has no delta
/// route, so an upgrade from an older release is always the full download.
#[test]
fn skips_the_delta_path() {
    let s = setup(Mode::Ok);
    let r = s.run(&["upgrade", "--version", "cli-v9.9.9"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(
        r.out.contains("(downloaded 7 B)") && !r.out.contains("patched"),
        "{}",
        r.out
    );
    assert!(s.server.requests().iter().all(|p| !p.ends_with(".patch")));
    assert_eq!(fs::read(&s.target).ok().as_deref(), Some(PAYLOAD));
}

/// A build with no release feed refuses instead of installing another
/// CLI's release over itself.
#[test]
fn no_release_feed_refuses_and_writes_nothing() {
    if option_env!("PEEKTRACE_RELEASE_BASE_URL").is_some() {
        return;
    }
    let s = setup(Mode::Ok);
    let t = s.target.to_string_lossy();
    for args in [&["upgrade", "--check"][..], &["upgrade"]] {
        let r = s
            .home
            .run_stdin(args, b"", &[("PEEKTRACE_UPGRADE_TARGET", &t)]);
        assert_eq!(r.code, 1, "{args:?}");
        assert!(
            r.err.starts_with("Upgrade is not available in this build"),
            "{}",
            r.err
        );
    }
    assert_eq!(fs::read(&s.target).ok().as_deref(), Some(OLD));
    assert_eq!(s.server.requests(), Vec::<String>::new());
}
