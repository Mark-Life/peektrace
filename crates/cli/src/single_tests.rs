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
