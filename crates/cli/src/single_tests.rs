//! Unit tests for `single.rs`.

use super::*;

#[test]
fn plans() {
    let err = |m: &str| Err(UserError::Invalid(m.to_owned()));
    assert_eq!(
        plan_target(Some("--cwd"), false, None, None),
        err("Missing value for --cwd.")
    );
    assert_eq!(
        plan_target(None, false, None, Some("p")),
        err("--parent needs an explicit subagent id.")
    );
    assert_eq!(
        plan_target(Some("x"), true, None, None),
        err("Pass either a session id or --latest/--cwd, not both.")
    );
    assert_eq!(
        plan_target(None, false, None, None),
        err("Pass a session id, --latest, or --cwd <dir>.")
    );
    assert_eq!(
        plan_target(Some("x"), false, None, None),
        Ok(Plan::Picked("x".to_owned()))
    );
    assert_eq!(
        plan_target(None, false, Some("/a"), None),
        Ok(Plan::Newest(Some("/a".to_owned())))
    );
}

#[test]
fn prefix_picks() {
    assert!(by_prefix("abc", "abcdef"));
    assert!(by_prefix("abc.jsonl", "abcdef"));
    assert!(!by_prefix("abcdef", "abcdef"));
    assert!(!by_prefix("abcdef.jsonl", "abcdef"));
    assert!(!by_prefix("/p/abcdef.jsonl", "abcdef"));
    assert!(!by_prefix("C:\\p\\abcdef.jsonl", "abcdef"));
}

#[test]
fn search_notes() {
    let target = |id: Option<&str>, parent: Option<&str>| TargetArgs {
        id: id.map(str::to_owned),
        parent: parent.map(str::to_owned),
        ..TargetArgs::default()
    };
    let note = vec!["codex sessions are not searched: not supported yet".to_owned()];
    let codex = [Agent::Codex];
    let quiet: Vec<String> = Vec::new();
    assert_eq!(search_note(&target(None, None), "s1", "s1", &codex), note);
    assert_eq!(
        search_note(&target(Some("s"), None), "s", "s1", &codex),
        note
    );
    assert_eq!(
        search_note(&target(Some("s1"), None), "s1", "s1", &codex),
        quiet
    );
    assert_eq!(
        search_note(&target(Some("a"), Some("s1")), "a", "a1", &codex),
        quiet
    );
    assert_eq!(search_note(&target(None, None), "s1", "s1", &[]), quiet);
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
fn verdicts_and_cells() {
    assert_eq!(verdict(0.1, 0.4), "Healthy");
    assert_eq!(verdict(0.5, 0.4), "Degrading");
    assert_eq!(verdict(0.7, 0.4), "Rotting");
    assert_eq!(tokens(1150.0), "1.1k");
    assert_eq!(tokens(999.4), "999");
    assert_eq!(tokens(2_500_000.0), "2.5M");
    assert_eq!(percent(0.115), "11.5%");
}

/// TS `verdict.test.ts`.
#[test]
fn verdict_bands_switch_at_the_dumb_zone_and_at_rotting() {
    assert_eq!(verdict(0.39, 0.4), "Healthy");
    assert_eq!(verdict(0.4, 0.4), "Degrading");
    assert_eq!(verdict(ROTTING_FRACTION - 0.01, 0.4), "Degrading");
    assert_eq!(verdict(ROTTING_FRACTION, 0.4), "Rotting");
}

/// TS `verdict.test.ts`.
#[test]
#[allow(clippy::float_cmp)]
fn fractions_are_zero_for_an_unknown_window() {
    assert_eq!(fraction_of(5.0, 0.0), 0.0);
    assert_eq!(fraction_of(2.0, 10.0), 0.2);
}

/// TS `sessions-filter.test.ts`: the `planTarget` and `pickTarget` cases
/// that need no listing.
#[test]
fn plans_like_pick_target() {
    let invalid = |r: Result<Plan, UserError>| matches!(r, Err(UserError::Invalid(_)));
    assert_eq!(plan_target(None, true, None, None), Ok(Plan::Newest(None)));
    assert_eq!(
        plan_target(None, true, Some("/w"), None),
        Ok(Plan::Newest(Some("/w".to_owned())))
    );
    assert!(invalid(plan_target(Some("abc"), true, None, None)));
    assert!(invalid(plan_target(Some("x"), false, Some("/w"), None)));
    assert_eq!(
        plan_target(Some("--cwd"), true, None, None),
        Err(UserError::Invalid("Missing value for --cwd.".to_owned()))
    );
    assert_eq!(
        plan_target(None, true, None, Some("p")),
        Err(UserError::Invalid(
            "--parent needs an explicit subagent id.".to_owned()
        ))
    );
    assert_eq!(
        plan_target(Some("x"), false, None, Some("p")),
        Ok(Plan::Picked("x".to_owned()))
    );
}
