use super::*;

fn header() -> SessionHeader {
    SessionHeader {
        id: "s-1".into(),
        agent: AgentId::Claude,
        path: "/home/dev/code/app/s-1.jsonl".into(),
        project: "-home-dev-code-app".into(),
        message_count: 3,
        size_bytes: 512,
        updated_at: String::new(),
        cwd: None,
        git_branch: None,
        model: None,
        title: None,
        started_at: None,
        cost_usd: None,
        source: None,
        source_label: None,
    }
}

fn with(f: impl FnOnce(&mut SessionHeader)) -> SessionHeader {
    let mut h = header();
    f(&mut h);
    h
}

fn ids(hs: &[SessionHeader], now: f64) -> Vec<&str> {
    sort_by_recent(hs, now)
        .into_iter()
        .map(|i| hs[i].id.as_str())
        .collect()
}

#[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
fn at(iso: &str) -> f64 {
    parse_date(iso).map_or(f64::NAN, |ms| ms as f64)
}

const FAR: f64 = 9e15;

#[test]
fn project_label_takes_the_last_folder() {
    let h = |cwd: &str| with(|h| h.cwd = Some(cwd.into()));
    assert_eq!(project_label(&h("/home/dev/code/app")), "app");
    assert_eq!(project_label(&h("/home/dev/code/app/")), "app");
    assert_eq!(project_label(&h("C:\\Users\\dev\\app")), "app");
    assert_eq!(project_label(&header()), "-home-dev-code-app");
}

#[test]
fn project_options_disambiguate_folders() {
    let hs = [
        with(|h| h.cwd = Some("/a/x/app".into())),
        with(|h| h.cwd = Some("/b/y/app".into())),
        with(|h| h.cwd = Some("/c/y/app".into())),
        with(|h| {
            h.agent = AgentId::Codex;
            h.cwd = Some("/a/x/app".into());
        }),
        with(|h| h.cwd = Some("/srv/tool".into())),
    ];
    let opts = project_options(&hs);
    let labels: Vec<&str> = opts.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, ["/b/y/app", "/c/y/app", "tool", "x/app"]);
}

#[test]
fn branch_hides_head_and_empty() {
    assert_eq!(visible_branch(Some("HEAD")), None);
    assert_eq!(visible_branch(Some("")), None);
    assert_eq!(
        visible_branch(Some("feature/search")),
        Some("feature/search")
    );
}

#[test]
fn sort_orders_by_last_activity() {
    let old = with(|h| {
        h.id = "old-start".into();
        h.started_at = Some("2026-01-01T00:00:00Z".into());
        h.updated_at = "2026-03-01T00:00:00Z".into();
    });
    let fresh = with(|h| {
        h.id = "new-start".into();
        h.started_at = Some("2026-02-01T00:00:00Z".into());
        h.updated_at = "2026-02-01T00:10:00Z".into();
    });
    assert_eq!(ids(&[fresh, old], FAR), ["old-start", "new-start"]);
    let utc = with(|h| {
        h.id = "utc".into();
        h.updated_at = "2026-05-01T10:30:00Z".into();
    });
    let offset = with(|h| {
        h.id = "offset".into();
        h.updated_at = "2026-05-01T11:00:00+02:00".into();
    });
    assert_eq!(ids(&[offset, utc], FAR), ["utc", "offset"]);
}

#[test]
fn sort_puts_undated_last_and_skips_future_stamps() {
    let none = with(|h| h.id = "none".into());
    let bad = with(|h| {
        h.id = "bad".into();
        h.updated_at = "not a date".into();
    });
    let dated = with(|h| {
        h.id = "dated".into();
        h.started_at = Some("2026-05-01T00:00:00Z".into());
    });
    assert_eq!(ids(&[none, bad, dated], FAR), ["dated", "bad", "none"]);
    let now = at("2026-06-01T12:00:00Z");
    let future = with(|h| {
        h.id = "future".into();
        h.started_at = Some("2026-01-01T00:00:00Z".into());
        h.updated_at = "2099-01-01T00:00:00Z".into();
    });
    let recent = with(|h| {
        h.id = "recent".into();
        h.updated_at = "2026-06-01T11:59:00Z".into();
    });
    let lost = with(|h| {
        h.id = "lost".into();
        h.updated_at = "2099-01-01T00:00:00Z".into();
    });
    assert_eq!(
        ids(&[lost, future, recent], now),
        ["recent", "future", "lost"]
    );
}

#[test]
fn sort_breaks_ties_by_id_in_locale_order() {
    let hs: Vec<_> = ["c", "B", "a", "b"]
        .into_iter()
        .map(|id| {
            with(|h| {
                h.id = id.into();
                h.updated_at = "2026-05-01T00:00:00Z".into();
            })
        })
        .collect();
    assert_eq!(ids(&hs, FAR), ["a", "b", "B", "c"]);
}

#[test]
fn live_window_and_skew() {
    let now = at("2026-06-01T12:00:00Z");
    let iso = |ms: f64| {
        #[expect(clippy::cast_possible_truncation, reason = "whole ms")]
        let ms = ms as i64;
        let day = day_key(ms);
        let rem = ms.rem_euclid(DAY_MS) / 1000;
        format!(
            "{day}T{:02}:{:02}:{:02}Z",
            rem / 3600,
            rem / 60 % 60,
            rem % 60
        )
    };
    let upd = |ms: f64| with(|h| h.updated_at = iso(ms));
    assert!(is_live(&upd(now - LIVE_MS + 1000.0), now));
    assert!(!is_live(&upd(now - LIVE_MS - 1000.0), now));
    assert!(is_live(&upd(now + SKEW_MS - 1000.0), now));
    assert!(!is_live(&upd(now + 3_600_000.0), now));
    assert!(is_live(
        &with(|h| h.started_at = Some(iso(now - 1000.0))),
        now
    ));
    assert!(!is_live(&header(), now));
}

#[test]
fn matches_query_and_facets() {
    let h = with(|h| {
        h.id = "abc123".into();
        h.title = Some("Fix the parser".into());
        h.git_branch = Some("HEAD".into());
        h.model = Some("model-a".into());
    });
    let f = Filters::default();
    assert!(matches(&h, &f));
    let q = |s: &str| Filters {
        query: s.into(),
        ..Filters::default()
    };
    assert!(matches(&h, &q("PARSER")));
    assert!(matches(&h, &q("abc1")));
    assert!(!matches(&h, &q("nope")));
    let project = |p: &str| Filters {
        project: Some(p.into()),
        ..Filters::default()
    };
    assert!(matches(&h, &project("-home-dev-code-app")));
    assert!(!matches(&h, &project("other")));
    assert!(matches(
        &h,
        &Filters {
            branch: Some("HEAD".into()),
            ..Filters::default()
        }
    ));
    assert!(!matches(
        &h,
        &Filters {
            model: Some("model-b".into()),
            ..Filters::default()
        }
    ));
    assert!(matches(&header(), &q("claude session")));
}

#[test]
fn active_days_span_and_range() {
    let spanning = with(|h| {
        h.started_at = Some("2026-11-01T09:00:00Z".into());
        h.updated_at = "2026-11-03T10:00:00Z".into();
    });
    assert_eq!(
        active_days(&spanning),
        ["2026-11-01", "2026-11-02", "2026-11-03"]
    );
    let bad = with(|h| {
        h.started_at = Some("garbage".into());
        h.updated_at = "2026-13-45T00:00:00Z".into();
    });
    assert_eq!(active_days(&bad), Vec::<String>::new());
    let range = |from: &str, to: Option<&str>| Filters {
        range: Some(DayRange {
            from: from.into(),
            to: to.map(Into::into),
        }),
        ..Filters::default()
    };
    assert!(matches(&spanning, &range("2026-11-02", None)));
    assert!(matches(&spanning, &range("2026-10-01", Some("2026-11-01"))));
    assert!(!matches(
        &spanning,
        &range("2026-11-04", Some("2026-11-09"))
    ));
}

#[test]
fn stamp_titles() {
    let h = with(|h| {
        h.started_at = Some("2026-11-01T09:05:00Z".into());
        h.updated_at = "2026-11-03T10:00:00Z".into();
    });
    assert_eq!(
        stamp_title(&h),
        "Last activity Nov 3, 10:00 · started Nov 1, 09:05"
    );
    assert_eq!(stamp_title(&header()), "unknown time");
    assert_eq!(fmt_stamp(Some("2026-13-01T00:00")), "—");
}

#[test]
fn hiding_filters_reset_only_what_hides() {
    let h = with(|h| {
        h.title = Some("Fix the parser".into());
        h.model = Some("model-a".into());
    });
    let f = Filters {
        query: "parser".into(),
        agent: Some(AgentId::Codex),
        model: Some("model-b".into()),
        ..Filters::default()
    };
    assert_eq!(
        hiding_filters(&h, &f),
        [FilterName::Agent, FilterName::Model]
    );
    assert_eq!(hiding_filters(&h, &Filters::default()), vec![]);
    assert_eq!(active_count(&f), 3);
}

// Ports of `apps/inspector/test/session-list-model.test.ts` cases the tests
// above do not already hold with the same fixture.

fn cwd_project(cwd: Option<&str>, project: &str) -> SessionHeader {
    with(|h| {
        h.cwd = cwd.map(Into::into);
        h.project = project.into();
    })
}

fn opts(pairs: &[(&str, &str)]) -> Vec<FilterOption> {
    pairs
        .iter()
        .map(|(value, label)| FilterOption {
            value: (*value).into(),
            label: (*label).into(),
        })
        .collect()
}

#[test]
fn project_label_falls_back_to_the_stored_project() {
    assert_eq!(
        project_label(&cwd_project(None, "-home-dev-code-app")),
        "-home-dev-code-app"
    );
    assert_eq!(project_label(&cwd_project(Some("/"), "root")), "root");
}

#[test]
fn project_label_splits_a_windows_path() {
    assert_eq!(
        project_label(&cwd_project(Some("C:\\work\\tools\\cli"), "x")),
        "cli"
    );
}

#[test]
fn project_options_key_by_cwd_and_label_by_folder() {
    let hs = [
        cwd_project(Some("/home/dev/code/web"), "p-web"),
        cwd_project(Some("/home/dev/code/api"), "p-api"),
        cwd_project(Some("/home/dev/code/api"), "p-api"),
    ];
    assert_eq!(
        project_options(&hs),
        opts(&[("/home/dev/code/api", "api"), ("/home/dev/code/web", "web")])
    );
}

#[test]
fn project_options_fall_back_to_the_slug() {
    assert_eq!(
        project_options(&[cwd_project(None, "p-web")]),
        opts(&[("p-web", "p-web")])
    );
}

#[test]
fn project_options_merge_harnesses_in_one_folder() {
    let hs = [
        cwd_project(Some("/w/acme/app"), "-w-acme-app"),
        cwd_project(Some("/w/acme/app"), "app"),
    ];
    assert_eq!(project_options(&hs), opts(&[("/w/acme/app", "app")]));
}

#[test]
fn project_options_add_the_parent_on_a_shared_name() {
    let hs = [
        cwd_project(Some("/home/dev/one/app"), "p-one"),
        cwd_project(Some("/home/dev/two/app"), "p-two"),
        cwd_project(Some("/home/dev/code/web"), "p-web"),
    ];
    assert_eq!(
        project_options(&hs),
        opts(&[
            ("/home/dev/one/app", "one/app"),
            ("/home/dev/two/app", "two/app"),
            ("/home/dev/code/web", "web"),
        ])
    );
}

#[test]
fn project_options_use_the_full_path_on_a_second_collision() {
    let hs = [
        cwd_project(Some("/a/one/app"), "p-a"),
        cwd_project(Some("/b/one/app"), "p-b"),
    ];
    assert_eq!(
        project_options(&hs),
        opts(&[("/a/one/app", "/a/one/app"), ("/b/one/app", "/b/one/app")])
    );
}

#[test]
fn visible_branch_hides_a_missing_branch() {
    assert_eq!(visible_branch(None), None);
}

#[test]
fn sort_breaks_ties_by_id() {
    let hs: Vec<_> = ["c", "a", "b"]
        .into_iter()
        .map(|id| {
            with(|h| {
                h.id = id.into();
                h.updated_at = "2026-05-01T00:00:00Z".into();
            })
        })
        .collect();
    assert_eq!(ids(&hs, FAR), ["a", "b", "c"]);
}

#[test]
fn matches_a_project_key_from_cwd() {
    let with_cwd = with(|h| h.cwd = Some("/home/dev/code/app".into()));
    let f = Filters {
        project: Some(project_key(&with_cwd).into()),
        ..Filters::default()
    };
    assert!(matches(&with_cwd, &f));
}

#[test]
fn active_days_of_one_stamp_and_of_none() {
    let one = with(|h| h.updated_at = "2026-11-08T10:00:00Z".into());
    assert_eq!(active_days(&one), ["2026-11-08"]);
    assert_eq!(active_days(&header()), Vec::<String>::new());
}

#[test]
fn active_days_skip_a_stamp_that_is_not_an_iso_day() {
    let days = |started: &str, updated: &str| {
        active_days(&with(|h| {
            h.started_at = Some(started.into());
            h.updated_at = updated.into();
        }))
    };
    assert_eq!(
        days("1/5/2025 10:00", "2025-01-05T10:00:00Z"),
        ["2025-01-05"]
    );
    assert_eq!(days("2025-01-05T10:00:00Z", "garbage"), ["2025-01-05"]);
    assert_eq!(days("bad", "worse"), Vec::<String>::new());
}

#[test]
fn range_matches_any_day_the_session_was_active_on() {
    let spanning = with(|h| {
        h.started_at = Some("2026-11-01T09:00:00Z".into());
        h.updated_at = "2026-11-03T10:00:00Z".into();
    });
    let day = |d: u8| format!("2026-11-{d:02}");
    let on = |from: u8, to: Option<u8>| {
        matches(
            &spanning,
            &Filters {
                range: Some(DayRange {
                    from: day(from),
                    to: to.map(day),
                }),
                ..Filters::default()
            },
        )
    };
    assert!(on(1, None));
    assert!(on(2, None));
    assert!(on(3, None));
    assert!(!on(4, None));
    assert!(!on(4, Some(8)));
    assert!(on(3, Some(8)));
}

#[test]
fn stamp_title_names_the_shown_stamp_and_adds_the_start() {
    let spanning = with(|h| {
        h.started_at = Some("2026-11-01T09:00:00Z".into());
        h.updated_at = "2026-11-03T10:00:00Z".into();
    });
    assert_eq!(
        stamp_title(&spanning),
        "Last activity Nov 3, 10:00 · started Nov 1, 09:00"
    );
    let started = with(|h| h.started_at = Some("2026-11-01T09:00:00Z".into()));
    assert_eq!(stamp_title(&started), "Last activity Nov 1, 09:00");
}

#[test]
fn hiding_filters_empty_when_the_session_passes() {
    let h = with(|h| h.model = Some("m-1".into()));
    assert_eq!(hiding_filters(&h, &Filters::default()), vec![]);
    let f = Filters {
        model: Some("m-1".into()),
        ..Filters::default()
    };
    assert_eq!(hiding_filters(&h, &f), vec![]);
}

#[test]
fn hiding_filters_reset_only_the_hiding_ones() {
    let h = with(|h| h.model = Some("m-1".into()));
    let f = Filters {
        agent: Some(AgentId::Codex),
        model: Some("m-1".into()),
        query: "zzz".into(),
        ..Filters::default()
    };
    let fix = hiding_filters(&h, &f);
    assert_eq!(fix, [FilterName::Query, FilterName::Agent]);
    let fixed = fix.into_iter().fold(f, Filters::without);
    assert!(matches(&h, &fixed));
}
