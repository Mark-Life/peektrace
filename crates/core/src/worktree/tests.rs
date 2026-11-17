use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::model::{HeaderCost, ProjectSlug, SessionId};

/// A fresh empty dir under the system temp dir, canonical so paths compare.
fn temp_dir(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("peektrace-wt-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    fs::canonicalize(&dir).expect("canonical")
}

fn s(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

/// A main checkout `<dir>/main` and a linked worktree `<dir>/wt/leaf` on
/// `head`, laid out as `git worktree add` leaves them.
fn linked_worktree(dir: &std::path::Path, head: &str) -> (PathBuf, PathBuf) {
    let main = dir.join("main");
    let admin = main.join(".git/worktrees/leaf");
    let wt = dir.join("wt/leaf");
    fs::create_dir_all(&admin).expect("admin");
    fs::create_dir_all(&wt).expect("wt");
    fs::write(admin.join("HEAD"), head).expect("head");
    fs::write(wt.join(".git"), format!("gitdir: {}\n", admin.display())).expect(".git");
    (main, wt)
}

fn none(_: &str) -> Option<Known> {
    None
}

#[test]
fn a_git_file_names_the_main_checkout_and_branch() {
    let dir = temp_dir("git");
    let (main, wt) = linked_worktree(&dir, "ref: refs/heads/feat/rows\n");
    let got = resolve(&format!("{}/", s(&wt)), none, None);
    assert_eq!(
        got,
        Some(Worktree {
            project: "main".into(),
            project_root: Some(s(&main)),
            path: s(&wt),
            branch: Some("feat/rows".into()),
        })
    );
    // A detached HEAD has no branch; the cache sees the HEAD change.
    let head = main.join(".git/worktrees/leaf/HEAD");
    fs::write(&head, "0123456789abcdef\n").expect("detach");
    let m = fs::metadata(&head)
        .and_then(|m| m.modified())
        .expect("mtime");
    fs::File::options()
        .write(true)
        .open(&head)
        .and_then(|f| f.set_modified(m + std::time::Duration::from_secs(60)))
        .expect("touch");
    assert_eq!(resolve(&s(&wt), none, None).and_then(|w| w.branch), None);
}

#[test]
fn a_relative_git_file_resolves_from_the_worktree() {
    let dir = temp_dir("rel");
    let (main, wt) = linked_worktree(&dir, "ref: refs/heads/x\n");
    fs::write(wt.join(".git"), "gitdir: ../../main/.git/worktrees/leaf\n").expect(".git");
    let got = read_linked(&s(&wt)).expect("linked");
    assert_eq!((got.root, got.branch), (s(&main), Some("x".into())));
}

#[test]
fn bare_repos_name_their_project_not_the_git_dir() {
    let dir = temp_dir("bare");
    for (common, root) in [(".bare", ""), ("app.git", "app")] {
        let admin = dir.join(common).join("worktrees/main");
        let wt = dir.join(format!("wt-{root}"));
        fs::create_dir_all(&admin).expect("admin");
        fs::create_dir_all(&wt).expect("wt");
        fs::write(admin.join("HEAD"), "ref: refs/heads/main\n").expect("head");
        fs::write(wt.join(".git"), format!("gitdir: {}\n", admin.display())).expect(".git");
        let want = if root.is_empty() {
            s(&dir)
        } else {
            s(&dir.join(root))
        };
        assert_eq!(read_linked(&s(&wt)).map(|l| l.root), Some(want), "{common}");
    }
}

#[test]
fn gitdir_paths_are_folded_lexically() {
    let dir = temp_dir("fold");
    let (main, wt) = linked_worktree(&dir, "ref: refs/heads/x\n");
    let dotted = format!("gitdir: {}/./wt/../main/.git/worktrees/leaf\n", s(&dir));
    fs::write(wt.join(".git"), dotted).expect(".git");
    assert_eq!(read_linked(&s(&wt)).map(|l| l.root), Some(s(&main)));
    fs::write(wt.join(".git"), "gitdir: ../../gone/.git/worktrees/leaf\n").expect(".git");
    assert_eq!(read_linked(&s(&wt)), None);
}

#[test]
fn main_checkouts_submodules_and_missing_dirs_are_not_worktrees() {
    let dir = temp_dir("not");
    let main = dir.join("main");
    fs::create_dir_all(main.join(".git")).expect("main");
    assert_eq!(resolve(&s(&main), none, None), None);
    let sub = dir.join("sub");
    fs::create_dir_all(&sub).expect("sub");
    fs::write(sub.join(".git"), "gitdir: ../main/.git/modules/sub\n").expect(".git");
    fs::create_dir_all(main.join(".git/modules/sub")).expect("modules");
    assert_eq!(resolve(&s(&sub), none, None), None);
    assert_eq!(resolve(&s(&dir.join("gone")), none, None), None);
    assert_eq!(resolve("", none, None), None);
}

#[test]
fn t3code_knowledge_wins_and_git_fills_the_branch() {
    let dir = temp_dir("known");
    let (_, wt) = linked_worktree(&dir, "ref: refs/heads/from-git\n");
    let wt_s = s(&wt);
    let known = |p: &str| {
        (p == wt_s).then(|| Known {
            project: "Peephole".into(),
            project_root: Some("/code/peephole".into()),
            branch: None,
        })
    };
    let got = resolve(&wt_s, known, None).expect("known");
    assert_eq!(got.project, "Peephole");
    assert_eq!(got.project_root.as_deref(), Some("/code/peephole"));
    assert_eq!(got.branch.as_deref(), Some("from-git"));
}

#[test]
fn t3_home_layout_is_the_last_resort() {
    let home = "/nowhere/.t3";
    let got = resolve(
        "/nowhere/.t3/worktrees/peephole/t3-1b686728/crates",
        none,
        Some(home),
    );
    assert_eq!(
        got,
        Some(Worktree {
            project: "peephole".into(),
            project_root: None,
            path: "/nowhere/.t3/worktrees/peephole/t3-1b686728".into(),
            branch: None,
        })
    );
    assert_eq!(
        resolve("/nowhere/.t3/worktrees/peephole", none, Some(home)),
        None
    );
    assert_eq!(
        resolve("/nowhere/.t3/worktreesx/p/l", none, Some(home)),
        None
    );
    assert_eq!(resolve("/elsewhere/p/l", none, Some(home)), None);
}

fn header(agent: Agent, project: &str, worktree: Option<Worktree>) -> SessionHeader {
    SessionHeader {
        id: SessionId::new("s"),
        agent,
        path: String::new(),
        project: ProjectSlug::new(project),
        message_count: 0,
        size_bytes: 0,
        updated_at: String::new(),
        cwd: None,
        git_branch: None,
        model: None,
        title: None,
        started_at: None,
        malformed_lines: None,
        cost: HeaderCost::default(),
        cost_parts: None,
        source: None,
        launcher: None,
        models: Vec::new(),
        worktree,
    }
}

#[test]
fn worktree_rows_print_their_main_checkout_project() {
    let wt = |root: Option<&str>| {
        Some(Worktree {
            project: "Peephole".into(),
            project_root: root.map(Into::into),
            path: "/h/.t3/worktrees/peephole/t3-1".into(),
            branch: None,
        })
    };
    let root = Some("/Users/a/Code/peep.hole");
    let cases = [
        (Agent::Claude, wt(root), "-Users-a-Code-peep-hole"),
        (Agent::Pi, wt(root), "--Users-a-Code-peep.hole--"),
        (Agent::Codex, wt(root), "peep.hole"),
        (Agent::Opencode, wt(root), "peep.hole"),
        (Agent::Claude, wt(None), "Peephole"),
        (Agent::Claude, None, "-own-slug"),
    ];
    for (agent, w, want) in cases {
        assert_eq!(row_project(&header(agent, "-own-slug", w)), want, "{agent}");
    }
}

#[test]
fn normalize_trims_trailing_separators_only() {
    assert_eq!(normalize("/a/b/"), "/a/b");
    assert_eq!(normalize("/a/b"), "/a/b");
    assert_eq!(normalize("/"), "/");
    assert_eq!(normalize(""), "");
}

#[test]
fn a_dir_inside_a_worktree_resolves_to_the_worktree() {
    let dir = temp_dir("subdir");
    let (main, wt) = linked_worktree(&dir, "ref: refs/heads/feat\n");
    let sub = wt.join("crates/core");
    fs::create_dir_all(&sub).expect("sub");
    let got = resolve(&s(&sub), none, None).expect("git");
    assert_eq!(got.path, s(&wt));
    assert_eq!(got.project_root, Some(s(&main)));
    assert_eq!(got.branch.as_deref(), Some("feat"));

    let wt_s = s(&wt);
    let known = |p: &str| {
        (p == wt_s).then(|| Known {
            project: "app".into(),
            project_root: Some("/code/app".into()),
            branch: Some("t3/x".into()),
        })
    };
    let got = resolve(&format!("{wt_s}/gone/deeper"), known, None).expect("known");
    assert_eq!(got.path, wt_s);
    assert_eq!(got.project_root.as_deref(), Some("/code/app"));
    assert_eq!(got.branch.as_deref(), Some("t3/x"));
}

#[test]
fn a_main_checkout_stops_the_walk_up() {
    let dir = temp_dir("nested");
    let (_, wt) = linked_worktree(&dir, "ref: refs/heads/feat\n");
    let inner = wt.join("vendor/repo");
    fs::create_dir_all(inner.join(".git")).expect("inner");
    assert_eq!(resolve(&s(&inner.join("src")), none, None), None);
}
