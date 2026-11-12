use super::*;

fn env() -> RootsEnv {
    RootsEnv {
        home: "/home/u".to_owned(),
        ..RootsEnv::default()
    }
}

fn entry(path: &str, label: Option<&str>) -> RootEntry {
    RootEntry {
        path: path.to_owned(),
        label: label.map(str::to_owned),
    }
}

#[test]
fn default_root_from_home() {
    let s = claude_sources(&env(), &[]);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].projects_root, "/home/u/.claude/projects");
}

#[test]
fn codex_and_pi_roots() {
    let root = |a: Agent, e: &RootsEnv, extras: &[RootEntry]| {
        sources(a, e, extras)
            .into_iter()
            .map(|s| (s.id, s.projects_root))
            .collect::<Vec<_>>()
    };
    let pair = |id: &str, r: &str| (id.to_owned(), r.to_owned());
    assert_eq!(
        root(Agent::Codex, &env(), &[]),
        [pair("codex", "/home/u/.codex/sessions")]
    );
    assert_eq!(
        root(Agent::Pi, &env(), &[]),
        [pair("pi", "/home/u/.pi/agent/sessions")]
    );
    let e = RootsEnv {
        codex_home: Some(String::new()),
        ..env()
    };
    assert_eq!(root(Agent::Codex, &e, &[]), [pair("codex", "sessions")]);
    let e = RootsEnv {
        codex_home: Some("/c".to_owned()),
        codex_sessions: Some("/s".to_owned()),
        pi_sessions: Some("/p".to_owned()),
        ..env()
    };
    let extras = [
        entry("/c", None),
        entry("~/work", Some("w")),
        entry("/s/..", None),
    ];
    assert_eq!(
        root(Agent::Codex, &e, &extras),
        [
            pair("codex", "/s"),
            pair("codex:c", "/c/sessions"),
            pair("codex:w", "/home/u/work/sessions"),
            pair("codex:..", "/sessions"),
        ]
    );
    assert_eq!(
        root(Agent::Pi, &e, &[entry("~/.pi", None), entry("/p2", None)]),
        [
            pair("pi", "/p"),
            pair("pi:.pi", "/home/u/.pi/agent/sessions"),
            pair("pi:p2", "/p2/agent/sessions"),
        ]
    );
    let s =
        parse_settings(r#"{"roots":{"codex":[{"path":"/a"}],"pi":[{"path":"/b","label":"x"}]}}"#);
    assert_eq!((s.codex.len(), s.pi.len(), s.claude.len()), (1, 1, 0));
}

#[test]
fn opencode_roots() {
    let data_dir = |e: &RootsEnv| sources(Agent::Opencode, e, &[])[0].projects_root.clone();
    assert_eq!(data_dir(&env()), "/home/u/.local/share/opencode");
    let xdg = RootsEnv {
        xdg_data_home: Some("/xdg".to_owned()),
        ..env()
    };
    assert_eq!(data_dir(&xdg), "/xdg/opencode");
    let empty = RootsEnv {
        opencode_data: Some(String::new()),
        ..xdg.clone()
    };
    assert_eq!(data_dir(&empty), "/xdg/opencode");
    let hook = RootsEnv {
        opencode_data: Some("/t/oc".to_owned()),
        ..xdg
    };
    assert_eq!(data_dir(&hook), "/t/oc");
    let settings = parse_settings(
        r#"{"roots":{"opencode":[{"path":"/work/oc"},{"path":"/t/oc"},{"path":"~/x","label":"x"}]}}"#,
    );
    let s = sources(Agent::Opencode, &hook, &settings.opencode);
    let got: Vec<_> = s
        .iter()
        .map(|s| (s.id.as_str(), s.home.as_str(), s.projects_root.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("opencode", "/t/oc", "/t/oc"),
            ("opencode:oc", "/work/oc", "/work/oc"),
            ("opencode:x", "/home/u/x", "/home/u/x"),
        ]
    );
}

#[test]
fn config_dir_and_override() {
    let e = RootsEnv {
        claude_config_dir: Some(" : /c/one/ :/c/two".to_owned()),
        ..env()
    };
    assert_eq!(claude_sources(&e, &[])[0].projects_root, "/c/one/projects");
    let e = RootsEnv {
        claude_projects: Some("/p//x/".to_owned()),
        ..e
    };
    assert_eq!(claude_sources(&e, &[])[0].projects_root, "/p//x/");
}

#[test]
fn extras_dedup_and_label() {
    let extras = [
        entry("~/.claude", None),
        entry(" ~/work/.claude ", Some(" work ")),
        entry("/x/default", None),
        entry("/y/work", None),
        entry("/z/work", None),
        entry("  ", Some("blank")),
        entry("/", None),
    ];
    let s = claude_sources(&env(), &extras);
    let got: Vec<_> = s
        .iter()
        .map(|s| (s.id.as_str(), s.projects_root.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("claude", "/home/u/.claude/projects"),
            ("claude:work", "/home/u/work/.claude/projects"),
            ("claude:default-2", "/x/default/projects"),
            ("claude:work-2", "/y/work/projects"),
            ("claude:work-3", "/z/work/projects"),
            ("claude:source", "/projects"),
        ]
    );
}

#[test]
fn windows_roots() {
    let e = RootsEnv {
        home: "C:\\Users\\u".to_owned(),
        style: PathStyle::Windows,
        ..RootsEnv::default()
    };
    assert_eq!(e.settings_path(), "C:\\Users\\u\\.peektrace\\settings.json");
    let s = claude_sources(
        &e,
        &[entry("~/work/.claude", None), entry("D:/x/", Some("x"))],
    );
    let roots: Vec<_> = s.iter().map(|s| s.projects_root.as_str()).collect();
    assert_eq!(
        roots,
        [
            "C:\\Users\\u\\.claude\\projects",
            "C:\\Users\\u\\work\\.claude\\projects",
            "D:\\x\\projects",
        ]
    );
    assert_eq!(s[1].label, ".claude");
    let e = RootsEnv {
        claude_config_dir: Some(" ; C:\\x\\.claude;D:\\y".to_owned()),
        ..e
    };
    assert_eq!(
        claude_sources(&e, &[])[0].projects_root,
        "C:\\x\\.claude\\projects"
    );
}

#[test]
fn settings_with_lone_surrogates_keep_roots() {
    let s = parse_settings(
        "{\"roots\":{\"claude\":[{\"path\":\"/w\\ud800\",\"label\":\"w\\ud800\"},\
         {\"path\":\"/x\u{10F801}\",\"label\":\"x\u{10F801}\"}]}}",
    );
    assert!(!s.unparseable);
    assert_eq!(
        s.claude,
        [
            entry("/w\u{FFFD}", Some("w\u{10F800}")),
            entry("/x\u{10F801}", Some("x\u{10F7FF}\u{10F801}")),
        ]
    );
    let strict =
        parse_settings("{\"roots\":{\"claude\":[{\"path\":\"/x\",\"label\":\"x\u{10F801}\"}]}}");
    assert_eq!(strict.claude, [entry("/x", Some("x\u{10F7FF}\u{10F801}"))]);
    let big = parse_settings(r#"{"n":1e400,"roots":{"claude":[{"path":1e400},{"path":"/a"}]}}"#);
    assert_eq!(big.claude, [entry("/a", None)]);
}

/// `JSON.parse` has no depth limit; `serde_json` stops at 128 levels.
#[test]
fn deep_settings_keep_roots() {
    let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
    let raw = format!(
        r#"{{"x":{deep},"roots":{{"claude":[{{"path":"/a","label":"work"}}]}},"y":{{"z":{deep}}}}}"#
    );
    assert_eq!(
        parse_settings(&raw),
        Settings {
            claude: vec![entry("/a", Some("work"))],
            ..Settings::default()
        }
    );
    let last = parse_settings(
        r#"{"roots":{"claude":[{"path":"/a"}]},"roots":{"claude":[{"path":"/b"}]}}"#,
    );
    assert_eq!(last.claude, [entry("/b", None)]);
}

#[test]
fn settings_parse_is_tolerant() {
    let raw = r#"{"roots":{"claude":[{"path":"~/a","label":"x"},{"path":1},{"path":"/b","label":null},{"path":"/c"}],"codex":5}}"#;
    assert_eq!(
        parse_settings(raw),
        Settings {
            claude: vec![entry("~/a", Some("x")), entry("/c", None)],
            ..Settings::default()
        }
    );
    assert_eq!(parse_settings(r#"{"roots":{"claude":{}}}"#).claude, []);
    assert_eq!(parse_settings(""), Settings::default());
}

/// Which inputs make TS log `ignoring unparseable settings`.
#[test]
fn unparseable_settings_match_ts() {
    let warns = [
        "not json",
        "{}",
        "[]",
        "5",
        r#"[{"roots":{}}]"#,
        r#"{"roots":null}"#,
        r#"{"roots":[]}"#,
        r#"{"roots":{"claude":null}}"#,
        r#"{"roots":{"codex":5}}"#,
        r#"{"roots":{"claude":[{"path":1}]}}"#,
        r#"{"roots":{"claude":[{"path":"/a","label":null}]}}"#,
    ];
    let quiet = [
        "",
        " \n ",
        r#"{"roots":{}}"#,
        r#"{"roots":{"foo":1}}"#,
        r#"{"roots":{"claude":[]}}"#,
        r#"{"roots":{"claude":[{"path":"/a"}],"codex":5}}"#,
        r#"{"roots":{"pi":[{"path":"/a"}]}}"#,
    ];
    for raw in warns {
        assert!(parse_settings(raw).unparseable, "{raw}");
    }
    for raw in quiet {
        assert!(!parse_settings(raw).unparseable, "{raw}");
    }
}
