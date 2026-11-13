//! The file watcher bumps the right version per changed path
//! (`watch.test.ts`, plus every agent's root).

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use peektrace_core::AgentRoots;
use peektrace_core::roots::Source;
use peektrace_core::watch::{WatchVersions, Watcher};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("peektrace-watch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("mkdir");
    d
}

fn source(root: &Path) -> Vec<Source> {
    vec![Source {
        id: "x".to_owned(),
        label: "default".to_owned(),
        home: root.to_string_lossy().into_owned(),
        projects_root: root.to_string_lossy().into_owned(),
    }]
}

/// Start a watcher, run `change`, and wait for a flush that satisfies `done`.
fn watch_until(
    roots: &AgentRoots,
    change: impl FnOnce(),
    done: impl Fn(WatchVersions) -> bool,
) -> WatchVersions {
    let (tx, rx) = mpsc::channel();
    let w = Watcher::start(roots, move |v| {
        let _ = tx.send(v);
    });
    std::thread::sleep(Duration::from_millis(300));
    change();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(v) if done(v) => break,
            _ => {}
        }
    }
    std::thread::sleep(Duration::from_millis(300));
    w.versions()
}

#[test]
fn memory_burst_bumps_memory_not_sessions() {
    let root = dir("memory");
    std::fs::create_dir_all(root.join("-p/memory")).expect("mkdir");
    let roots = AgentRoots {
        claude: source(&root),
        ..AgentRoots::default()
    };
    let v = watch_until(
        &roots,
        || {
            for i in 0..5 {
                std::fs::write(root.join(format!("-p/memory/m{i}.md")), "x").expect("write");
            }
        },
        |v| v.memory >= 1,
    );
    assert!(v.memory >= 1);
    assert_eq!(v.sessions, 0);
}

#[test]
fn transcripts_bump_sessions_for_every_agent() {
    let claude = dir("claude");
    let codex = dir("codex");
    let opencode = dir("opencode");
    let roots = AgentRoots {
        claude: source(&claude),
        codex: source(&codex),
        opencode: source(&opencode),
        ..AgentRoots::default()
    };
    for (name, path) in [
        ("claude", claude.join("-p/s.jsonl")),
        ("codex", codex.join("2026/r.jsonl")),
        ("opencode", opencode.join("opencode.db-wal")),
    ] {
        let v = watch_until(
            &roots,
            || {
                std::fs::create_dir_all(path.parent().expect("dir")).expect("mkdir");
                std::fs::write(&path, "{}\n").expect("write");
            },
            |v| v.sessions >= 1,
        );
        assert!(v.sessions >= 1, "{name}: {v:?}");
        assert_eq!(v.memory, 0, "{name}");
    }
}

#[test]
fn missing_root_reports_zeros() {
    let roots = AgentRoots {
        claude: source(Path::new("/nonexistent/peektrace-watch")),
        ..AgentRoots::default()
    };
    let w = Watcher::start(&roots, |_| {});
    assert_eq!(w.versions(), WatchVersions::default());
}

fn wait_for(w: &Watcher, done: impl Fn(WatchVersions) -> bool) -> WatchVersions {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline && !done(w.versions()) {
        std::thread::sleep(Duration::from_millis(50));
    }
    // Let a split burst land its last flush.
    std::thread::sleep(Duration::from_millis(300));
    w.versions()
}

// watch.test.ts: one watcher, a memory burst then a new transcript, each
// advancing only its own scope.
#[test]
fn emits_scope_classified_bumps_on_real_fs_writes() {
    const SLUG: &str = "-Users-demo-proj";
    let root = dir("sequence");
    let mem = root.join(SLUG).join("memory");
    std::fs::create_dir_all(&mem).expect("mkdir");
    let note = mem.join("note.md");
    std::fs::write(
        &note,
        "---\nname: note\ndescription: seed\ntype: user\n---\nbody\n",
    )
    .expect("seed");
    let roots = AgentRoots {
        claude: source(&root),
        ..AgentRoots::default()
    };
    let w = Watcher::start(&roots, |_| {});
    std::thread::sleep(Duration::from_millis(500));
    let before = w.versions();
    for i in 0..3 {
        std::fs::write(
            &note,
            format!("---\nname: note\ndescription: seed\ntype: user\n---\nedit {i}\n"),
        )
        .expect("edit");
    }
    let after_memory = wait_for(&w, |v| v.memory > before.memory);
    assert!(after_memory.memory > before.memory, "{after_memory:?}");
    assert_eq!(after_memory.sessions, before.sessions);
    std::fs::write(
        root.join(SLUG)
            .join("33333333-3333-4333-8333-333333333333.jsonl"),
        "{\"type\":\"user\"}\n",
    )
    .expect("session");
    let after_session = wait_for(&w, |v| v.sessions > after_memory.sessions);
    assert!(
        after_session.sessions > after_memory.sessions,
        "{after_session:?}"
    );
    assert_eq!(after_session.memory, after_memory.memory);
}
