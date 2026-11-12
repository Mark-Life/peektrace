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
