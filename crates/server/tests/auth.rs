//! Host, Origin and token guards (`serve-auth.test.ts` server cases).

mod common;

use common::{Options, start};

const TOKEN: &str = "abcdefghijklmnopqrstuvwxyz0123456789";

fn authed() -> common::TestServer {
    start(&Options {
        name: "auth",
        host: "0.0.0.0",
        token: Some(TOKEN),
        ..Options::default()
    })
}

#[test]
fn everything_needs_the_token() {
    let s = authed();
    for path in ["/api/server", "/", "/health"] {
        let a = s.get(path);
        assert_eq!(a.status, 401, "{path}");
        assert_eq!(a.header("cache-control"), Some("no-store"));
        assert_eq!(a.header("referrer-policy"), Some("no-referrer"));
    }
    let wrong = s.call("GET", "/health", &[("Authorization", "Bearer nope")], None);
    assert_eq!(wrong.status, 401);
    assert_eq!(s.get("/?token=nope").status, 401);
}

#[test]
fn bearer_cookie_and_redirect() {
    let s = authed();
    let bearer = format!("Bearer {TOKEN}");
    assert_eq!(
        s.call("GET", "/health", &[("Authorization", &bearer)], None)
            .status,
        200
    );
    let r = s.get(&format!("/sessions?x=1&token={TOKEN}&y=2"));
    assert_eq!(r.status, 302);
    assert_eq!(r.header("location"), Some("/sessions?x=1&y=2"));
    let cookie = r.header("set-cookie").expect("cookie").to_owned();
    assert_eq!(
        cookie,
        format!(
            "peektrace_token_{}={TOKEN}; Path=/; HttpOnly; SameSite=Lax",
            s.port
        )
    );
    let jar = cookie.split(';').next().unwrap_or_default().to_owned();
    assert_eq!(
        s.call("GET", "/api/server", &[("Cookie", &jar)], None)
            .status,
        200
    );
    let again = s.call(
        "GET",
        &format!("/?token={TOKEN}"),
        &[("Cookie", &jar)],
        None,
    );
    assert_eq!(again.status, 302);
}

#[test]
fn cookie_login_round_trips_a_token_with_symbols() {
    let token = "pass%41word; with symbols and \u{e9} 0123456789";
    let s = start(&Options {
        name: "auth-symbols",
        host: "0.0.0.0",
        token: Some(token),
        ..Options::default()
    });
    let link = format!(
        "/?token={}",
        peektrace_server::auth::encode_uri_component(token)
    );
    let r = s.get(&link);
    assert_eq!(r.status, 302);
    let cookie = r.header("set-cookie").expect("cookie").to_owned();
    assert_eq!(
        cookie,
        format!(
            "peektrace_token_{}=pass%2541word%3B%20with%20symbols%20and%20%C3%A9%200123456789; Path=/; HttpOnly; SameSite=Lax",
            s.port
        )
    );
    let jar = cookie.split(';').next().unwrap_or_default().to_owned();
    assert_eq!(
        s.call("GET", "/api/server", &[("Cookie", &jar)], None)
            .status,
        200
    );
}

#[test]
fn any_host_with_the_token_and_origin_must_match_host() {
    let s = authed();
    let bearer = format!("Bearer {TOKEN}");
    let lan = [
        ("Host", "192.168.1.5:9"),
        ("Authorization", bearer.as_str()),
    ];
    assert_eq!(s.call("GET", "/health", &lan, None).status, 200);
    let host = format!("127.0.0.1:{}", s.port);
    let https = format!("https://{host}");
    let ok = [
        ("Origin", https.as_str()),
        ("Authorization", bearer.as_str()),
    ];
    assert_eq!(s.call("GET", "/api/server", &ok, None).status, 200);
    let evil = [
        ("Origin", "http://evil.com"),
        ("Authorization", bearer.as_str()),
    ];
    assert_eq!(s.call("GET", "/api/server", &evil, None).status, 403);
}

#[test]
fn loopback_is_strict_about_host_and_origin() {
    let s = start(&Options {
        name: "loopback",
        ..Options::default()
    });
    assert_eq!(
        s.call("GET", "/health", &[("Host", "evil.com")], None)
            .status,
        403
    );
    let other = format!("localhost:{}", s.port);
    assert_eq!(
        s.call("GET", "/health", &[("Host", &other)], None).status,
        200
    );
    let evil = [("Origin", "http://evil.com")];
    assert_eq!(s.call("GET", "/api/server", &evil, None).status, 403);
    assert_eq!(s.call("GET", "/health", &evil, None).status, 200);
    let own = format!("http://127.0.0.1:{}", s.port);
    assert_eq!(
        s.call("GET", "/api/server", &[("Origin", &own)], None)
            .status,
        200
    );
    assert_eq!(s.get("/health").header("referrer-policy"), None);
}

// serve-auth.test.ts "authed server" and "loopback server" cases, one each.

#[test]
fn no_credentials_gives_401_with_the_token_hint() {
    let s = authed();
    let a = s.get("/");
    assert_eq!(a.status, 401);
    assert!(a.body.contains("tui footer"), "{}", a.body);
}

#[test]
fn right_token_redirects_sets_an_http_only_cookie_and_keeps_other_params() {
    let s = authed();
    let r = s.get(&format!("/?titlebar=inset&token={TOKEN}"));
    assert_eq!(r.status, 302);
    assert_eq!(r.header("location"), Some("/?titlebar=inset"));
    assert_eq!(r.header("referrer-policy"), Some("no-referrer"));
    let cookie = r.header("set-cookie").unwrap_or_default().to_owned();
    for part in [
        format!("peektrace_token_{}={TOKEN}", s.port),
        "HttpOnly".to_owned(),
        "SameSite=Lax".to_owned(),
        "Path=/".to_owned(),
    ] {
        assert!(cookie.contains(&part), "{cookie} lacks {part}");
    }
    let pair = cookie.split(';').next().unwrap_or_default().to_owned();
    for path in ["/api/server", "/health"] {
        let a = s.call("GET", path, &[("Cookie", &pair)], None);
        assert_eq!(a.status, 200, "{path}");
    }
}

#[test]
fn right_bearer_gives_200_on_the_api() {
    let s = authed();
    let bearer = format!("Bearer {TOKEN}");
    let a = s.call("GET", "/api/server", &[("Authorization", &bearer)], None);
    assert_eq!(a.status, 200);
}

#[test]
fn a_lan_style_host_passes_with_a_token_and_fails_without() {
    let s = authed();
    let host = format!("192.0.2.10:{}", s.port);
    let bearer = format!("Bearer {TOKEN}");
    let ok = [("Authorization", bearer.as_str()), ("Host", host.as_str())];
    assert_eq!(s.call("GET", "/api/server", &ok, None).status, 200);
    let none = [("Host", host.as_str())];
    assert_eq!(s.call("GET", "/api/server", &none, None).status, 401);
}

#[test]
fn an_https_origin_matching_host_passes() {
    let s = authed();
    let bearer = format!("Bearer {TOKEN}");
    let headers = [
        ("Authorization", bearer.as_str()),
        ("Host", "box.example"),
        ("Origin", "https://box.example"),
    ];
    assert_eq!(s.call("GET", "/api/server", &headers, None).status, 200);
}

#[test]
fn a_valid_cookie_plus_token_still_redirects() {
    let s = authed();
    let cookie = format!("peektrace_token_{}={TOKEN}", s.port);
    let r = s.call(
        "GET",
        &format!("/?token={TOKEN}"),
        &[("Cookie", &cookie)],
        None,
    );
    assert_eq!(r.status, 302);
    assert_eq!(r.header("location"), Some("/"));
}

#[test]
fn origin_must_equal_the_request_host() {
    let s = authed();
    let host = format!("192.0.2.10:{}", s.port);
    let origin = format!("http://{host}");
    let bearer = format!("Bearer {TOKEN}");
    let good = [
        ("Authorization", bearer.as_str()),
        ("Host", host.as_str()),
        ("Origin", origin.as_str()),
    ];
    assert_eq!(s.call("GET", "/api/server", &good, None).status, 200);
    let bad = [
        ("Authorization", bearer.as_str()),
        ("Host", host.as_str()),
        ("Origin", "http://evil.example"),
    ];
    assert_eq!(s.call("GET", "/api/server", &bad, None).status, 403);
}

#[test]
fn loopback_serves_with_no_credentials_and_a_strict_host_guard() {
    let s = start(&Options {
        name: "loopback-strict",
        ..Options::default()
    });
    assert_eq!(s.get("/api/server").status, 200);
    let health = s.get("/health");
    assert_eq!(health.status, 200);
    assert_eq!(health.header("referrer-policy"), None);
    let rebound = format!("evil.example:{}", s.port);
    let a = s.call("GET", "/api/server", &[("Host", &rebound)], None);
    assert_eq!(a.status, 403);
}
