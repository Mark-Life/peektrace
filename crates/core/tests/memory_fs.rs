//! Atomic writes, compare-and-swap and path containment (port of
//! `packages/core/test/fs.test.ts`).

mod common;

use std::fs;
use std::time::{Duration, SystemTime};

use common::TempDir;
use peektrace_core::memory::fs::{WriteError, atomic_write, read_text, stat_mtime};

fn bump_mtime(path: &str) {
    let future = SystemTime::now() + Duration::from_secs(10);
    fs::File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(future))
        .expect("set mtime");
}

#[test]
fn writes_content_atomically_into_a_real_temp_dir() {
    let d = TempDir::new("memory-fs-write");
    let path = d.join("note.md");
    atomic_write(&path, "hello", None, &[d.str()]).expect("write");
    assert_eq!(fs::read_to_string(&path).expect("read"), "hello");
    assert_eq!(read_text(&path).expect("read"), "hello");
}

#[test]
fn leaves_no_temp_files_behind() {
    let d = TempDir::new("memory-fs-clean");
    atomic_write(&d.join("clean.md"), "x", None, &[d.str()]).expect("write");
    let stray: Vec<String> = fs::read_dir(d.path())
        .expect("dir")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp-"))
        .collect();
    assert!(stray.is_empty(), "{stray:?}");
}

#[test]
fn succeeds_when_expected_mtime_matches_rejects_a_stale_mtime() {
    let d = TempDir::new("memory-fs-cas");
    let roots = vec![d.str()];
    let path = d.join("cas.md");
    atomic_write(&path, "v1", None, &roots).expect("write");
    let fresh = stat_mtime(&path).expect("stat");
    atomic_write(&path, "v2", Some(fresh), &roots).expect("cas write");
    fs::write(&path, "external").expect("write");
    bump_mtime(&path);
    let stale = atomic_write(&path, "v3", Some(fresh), &roots);
    assert!(matches!(stale, Err(WriteError::Changed { .. })));
    assert_eq!(fs::read_to_string(&path).expect("read"), "external");
    let gone = atomic_write(&d.join("gone.md"), "x", Some(fresh), &roots);
    assert_eq!(
        gone.err().map(|e| e.to_string()),
        Some(format!(
            "File changed on disk (missing): {}. Reload and retry.",
            d.join("gone.md")
        ))
    );
}

#[test]
fn rejects_a_path_escaping_every_agent_root_and_the_temp_dir() {
    let d = TempDir::new("memory-fs-escape");
    let outside = TempDir::new("memory-fs-outside");
    let r = atomic_write(&outside.join("escape.txt"), "nope", None, &[d.str()]);
    assert!(matches!(r, Err(WriteError::OutsideRoots { .. })));
    assert!(!fs::exists(outside.join("escape.txt")).unwrap_or(true));
}
