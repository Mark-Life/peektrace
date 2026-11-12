//! `list_sessions_cached`: unchanged files reuse their header, changed or
//! unreadable ones are read again.

mod common;

use std::fs;
use std::time::SystemTime;

use peektrace_core::header_cache::HeaderCache;
use peektrace_core::{ListFilter, list_sessions, list_sessions_cached};

fn transcript(cwd: &str) -> String {
    format!(
        "{{\"type\":\"user\",\"sessionId\":\"s1\",\"cwd\":\"{cwd}\",\"timestamp\":\"2026-01-01T00:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"hi\"}}}}\n"
    )
}

#[test]
fn unchanged_files_reuse_their_header() {
    let dir = std::env::temp_dir().join(format!("peektrace-header-cache-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let projects = dir.join("projects");
    let file = projects.join("-proj").join("s1.jsonl");
    fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    fs::write(&file, transcript("/a")).expect("write");
    let mtime: SystemTime = fs::metadata(&file)
        .and_then(|m| m.modified())
        .expect("mtime");
    let roots = common::claude_roots(&common::path_str(&dir), &common::path_str(&projects));
    let filter = ListFilter::default();
    let cache = HeaderCache::default();
    let cwd = |l: &peektrace_core::SessionListing| l.headers[0].cwd.clone();

    assert_eq!(
        cwd(&list_sessions_cached(&roots, &filter, &cache)),
        Some("/a".to_owned())
    );

    // Same size, same mtime: the cached header stands.
    fs::write(&file, transcript("/b")).expect("rewrite");
    fs::File::options()
        .write(true)
        .open(&file)
        .and_then(|f| f.set_modified(mtime))
        .expect("set mtime");
    assert_eq!(
        cwd(&list_sessions_cached(&roots, &filter, &cache)),
        Some("/a".to_owned())
    );
    assert_eq!(cwd(&list_sessions(&roots, &filter)), Some("/b".to_owned()));

    // A new size is read again.
    fs::write(&file, format!("{}{}", transcript("/b"), transcript("/b"))).expect("append");
    assert_eq!(
        cwd(&list_sessions_cached(&roots, &filter, &cache)),
        Some("/b".to_owned())
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).expect("chmod");
        let locked = list_sessions_cached(&roots, &filter, &cache);
        // Root reads a 000 file anyway.
        let still_reads = fs::File::open(&file).is_ok();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).expect("chmod back");
        if !still_reads {
            assert_eq!(locked.unreadable.len(), 1);
            assert_eq!(locked.headers.len(), 0);
        }
    }
    let _ = fs::remove_dir_all(&dir);
}
