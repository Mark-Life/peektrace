//! UI files and API answers both go out gzipped when the client asks.

mod common;

use std::io::{Read as _, Write as _};
use std::net::TcpStream;

use common::{Options, TestServer, start};

/// Response headers of a raw `GET` that asks for gzip. `ureq` drops
/// `content-encoding` when its `gzip` feature is on, which the workspace
/// turns on through the cli crate.
fn gzip_headers(s: &TestServer, path: &str) -> Vec<(String, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", s.port)).expect("connect");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n",
        s.port
    )
    .expect("request");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("response");
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("header end");
    String::from_utf8_lossy(&raw[..end])
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect()
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

#[test]
fn ui_assets_are_gzipped_like_the_api() {
    let s = start(&Options {
        name: "gzip",
        ..Options::default()
    });
    let assets = s.ui.join("assets");
    std::fs::create_dir_all(&assets).expect("mkdir assets");
    std::fs::write(assets.join("app.js"), "console.log(1);\n".repeat(400)).expect("asset");

    let asset = gzip_headers(&s, "/assets/app.js");
    assert_eq!(header(&asset, "content-encoding"), Some("gzip"));
    assert_eq!(
        header(&asset, "cache-control"),
        Some("public, max-age=31536000, immutable")
    );

    let plain = s.call(
        "GET",
        "/assets/app.js",
        &[("accept-encoding", "identity")],
        None,
    );
    assert_eq!(plain.header("content-encoding"), None);
    assert_eq!(plain.body.len(), 6400);

    let api = gzip_headers(&s, "/api/sessions");
    assert_eq!(header(&api, "content-encoding"), Some("gzip"));

    let small = gzip_headers(&s, "/");
    assert_eq!(header(&small, "content-encoding"), None);
}

#[test]
fn compressed_formats_go_out_as_is() {
    let s = start(&Options {
        name: "gzip-binary",
        ..Options::default()
    });
    let assets = s.ui.join("assets");
    std::fs::create_dir_all(&assets).expect("mkdir assets");
    let big = vec![b'a'; 8192];
    for name in ["a.png", "a.jpg", "a.woff2", "a.svg"] {
        std::fs::write(assets.join(name), &big).expect("asset");
    }
    for name in ["a.png", "a.jpg", "a.woff2"] {
        let h = gzip_headers(&s, &format!("/assets/{name}"));
        assert_eq!(header(&h, "content-encoding"), None, "{name}");
    }
    let svg = gzip_headers(&s, "/assets/a.svg");
    assert_eq!(header(&svg, "content-encoding"), Some("gzip"));
}
