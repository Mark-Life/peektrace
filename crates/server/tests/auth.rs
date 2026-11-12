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
