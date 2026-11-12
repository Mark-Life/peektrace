//! Unit tests for `single.rs`.

use super::*;

#[test]
fn plans() {
    let err = |m: &str| Err(UserError::Invalid(m.to_owned()));
    assert_eq!(
        plan_target(Some("--cwd"), false, None, None, &CLI_TARGET),
        err("Missing value for --cwd.")
    );
    assert_eq!(
        plan_target(None, false, None, Some("p"), &CLI_TARGET),
        err("--parent needs an explicit subagent id.")
    );
    assert_eq!(
        plan_target(Some("x"), true, None, None, &CLI_TARGET),
        err("Pass either a session id or --latest/--cwd, not both.")
    );
    assert_eq!(
        plan_target(None, false, None, None, &CLI_TARGET),
        err("Pass a session id, --latest, or --cwd <dir>.")
    );
    assert_eq!(
        plan_target(Some("x"), false, None, None, &CLI_TARGET),
        Ok(Plan::Picked("x".to_owned()))
    );
    assert_eq!(
        plan_target(None, false, Some("/a"), None, &CLI_TARGET),
        Ok(Plan::Newest(Some("/a".to_owned())))
    );
}

#[test]
fn formats() {
    assert_eq!(pick_format(None, false, Some("x.HTM")), ExportFormat::Html);
    assert_eq!(pick_format(None, false, Some(".md")), ExportFormat::Md);
    assert_eq!(pick_format(None, true, Some("x.html")), ExportFormat::Json);
    assert_eq!(
        pick_format(Some(ExportFormat::Md), true, None),
        ExportFormat::Md
    );
    assert_eq!(pick_format(None, false, Some("a.json")), ExportFormat::Json);
}

#[test]
fn cells() {
    assert_eq!(tokens(1150.0), "1.1k");
    assert_eq!(tokens(999.4), "999");
    assert_eq!(tokens(2_500_000.0), "2.5M");
    assert_eq!(percent(0.115), "11.5%");
}

/// TS `sessions-filter.test.ts`: the `planTarget` and `pickTarget` cases
/// that need no listing.
#[test]
fn plans_like_pick_target() {
    let invalid = |r: Result<Plan, UserError>| matches!(r, Err(UserError::Invalid(_)));
    assert_eq!(
        plan_target(None, true, None, None, &CLI_TARGET),
        Ok(Plan::Newest(None))
    );
    assert_eq!(
        plan_target(None, true, Some("/w"), None, &CLI_TARGET),
        Ok(Plan::Newest(Some("/w".to_owned())))
    );
    assert!(invalid(plan_target(
        Some("abc"),
        true,
        None,
        None,
        &CLI_TARGET
    )));
    assert!(invalid(plan_target(
        Some("x"),
        false,
        Some("/w"),
        None,
        &CLI_TARGET
    )));
    assert_eq!(
        plan_target(Some("--cwd"), true, None, None, &CLI_TARGET),
        Err(UserError::Invalid("Missing value for --cwd.".to_owned()))
    );
    assert_eq!(
        plan_target(None, true, None, Some("p"), &CLI_TARGET),
        Err(UserError::Invalid(
            "--parent needs an explicit subagent id.".to_owned()
        ))
    );
    assert_eq!(
        plan_target(Some("x"), false, None, Some("p"), &CLI_TARGET),
        Ok(Plan::Picked("x".to_owned()))
    );
}

/// `--cwd` picks from the cache when one is passed, so a long-lived MCP
/// process does not re-read every header per call.
#[test]
fn newest_reuses_the_header_cache() {
    use std::fs;
    let home = std::env::temp_dir().join(format!("peektrace-single-cache-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let file = home.join(".claude/projects/-proj/s1.jsonl");
    fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    let transcript = |cwd: &str| {
        format!(
            "{{\"type\":\"user\",\"sessionId\":\"s1\",\"cwd\":\"{cwd}\",\"timestamp\":\"2026-01-01T00:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"hi\"}}}}\n"
        )
    };
    fs::write(&file, transcript("/a")).expect("write");
    let mtime = fs::metadata(&file)
        .and_then(|m| m.modified())
        .expect("mtime");
    let ctx = Context {
        roots_env: roots::RootsEnv {
            home: home.to_string_lossy().into_owned(),
            ..Default::default()
        },
        settings: None,
        cwd: None,
        now_ms: 0.0,
    };
    let globals = Globals {
        json: true,
        pretty: false,
        read_only: true,
    };
    let target = TargetArgs {
        id: None,
        latest: false,
        cwd: Some("/a".to_owned()),
        parent: None,
    };
    let cache = HeaderCache::default();
    let run = |cache| analyze(&target, &CLI_TARGET, globals, &ctx, None, cache);
    assert!(run(Some(&cache)).is_ok());

    // Same size and mtime: only the cached header still says `/a`.
    fs::write(&file, transcript("/b")).expect("rewrite");
    fs::File::options()
        .write(true)
        .open(&file)
        .and_then(|f| f.set_modified(mtime))
        .expect("set mtime");
    let cached = run(Some(&cache));
    let fresh = run(None);
    let _ = fs::remove_dir_all(&home);
    assert!(cached.is_ok(), "{cached:?}");
    assert_eq!(
        fresh,
        Err(UserError::Invalid("No sessions found under /a.".to_owned()))
    );
}
