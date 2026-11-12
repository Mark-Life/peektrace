//! UI files and API answers both go out gzipped when the client asks.

mod common;

use common::{Options, start};

const GZIP: (&str, &str) = ("accept-encoding", "gzip");

#[test]
fn ui_assets_are_gzipped_like_the_api() {
    let s = start(&Options {
        name: "gzip",
        ..Options::default()
    });
    let assets = s.ui.join("assets");
    std::fs::create_dir_all(&assets).expect("mkdir assets");
    std::fs::write(assets.join("app.js"), "console.log(1);\n".repeat(400)).expect("asset");

    let asset = s.call("GET", "/assets/app.js", &[GZIP], None);
    assert_eq!(asset.status, 200);
    assert_eq!(asset.header("content-encoding"), Some("gzip"));
    assert_eq!(
        asset.header("cache-control"),
        Some("public, max-age=31536000, immutable")
    );

    let plain = s.get("/assets/app.js");
    assert_eq!(plain.header("content-encoding"), None);
    assert_eq!(plain.body.len(), 6400);

    let api = s.call("GET", "/api/sessions", &[GZIP], None);
    assert_eq!(api.header("content-encoding"), Some("gzip"));

    let small = s.call("GET", "/", &[GZIP], None);
    assert_eq!(small.header("content-encoding"), None);
}
