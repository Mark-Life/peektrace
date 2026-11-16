//! `peektrace pricing` and `pricing update --from`: no test touches the
//! network.

mod common;

use std::fs;

use common::Home;

const API: &str = r#"{
  "anthropic": {"models": {"claude-opus-5-5": {"cost": {"input": 4, "output": 20, "cache_read": 0.2, "cache_write": 5}}}},
  "openai": {"models": {"gpt-5.5": {"cost": {"input": 5, "output": 30, "cache_read": 0.5}}}}
}"#;

#[test]
fn shows_the_built_in_table_when_nothing_was_downloaded() {
    let h = Home::new("pricing-show");
    let r = h.run(&["pricing"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.starts_with("Prices: models.dev 20"), "{}", r.out);
    assert!(r.out.contains(", built-in\n"), "{}", r.out);
    assert!(r.out.contains("prices.json (none"), "{}", r.out);
}

#[test]
fn update_writes_the_data_dir_and_a_newer_table_is_used() {
    let h = Home::new("pricing-update");
    let api = h.dir().join("api.json");
    fs::write(&api, API).expect("write api");
    let r = h.run(&["pricing", "update", "--from", &api.to_string_lossy()]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.starts_with("Wrote 2 models"), "{}", r.out);
    let local = h.dir().join(".peektrace/prices.json");
    let text = fs::read_to_string(&local).expect("table written");
    assert!(text.contains("\"id\":\"gpt-5.5\""), "{text}");

    let newer = text.replacen(
        &text[text.find("\"fetched\"").expect("date")..][..23],
        "\"fetched\": \"2999-01-01\"",
        1,
    );
    fs::write(&local, newer).expect("write newer");
    let r = h.run(&["pricing"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(
        r.out
            .starts_with("Prices: models.dev 2999-01-01, downloaded\n"),
        "{}",
        r.out
    );
}

#[test]
fn update_with_output_leaves_an_unchanged_table_alone() {
    let h = Home::new("pricing-output");
    let api = h.dir().join("api.json");
    fs::write(&api, API).expect("write api");
    let out = h.dir().join("table.json");
    let args = [
        "pricing",
        "update",
        "--from",
        &api.to_string_lossy(),
        "--output",
        &out.to_string_lossy(),
    ]
    .map(str::to_owned);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    assert_eq!(h.run(&args).code, 0);
    let r = h.run(&args);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.starts_with("Prices unchanged"), "{}", r.out);
}

#[test]
fn update_refuses_read_only_and_bad_input() {
    let h = Home::new("pricing-refuse");
    let r = h.run(&["--read-only", "pricing", "update"]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("--read-only is set"), "{}", r.err);
    let bad = h.dir().join("bad.json");
    fs::write(&bad, "{}").expect("write");
    let r = h.run(&["pricing", "update", "--from", &bad.to_string_lossy()]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("Not a usable models.dev table"), "{}", r.err);
    assert!(!h.dir().join(".peektrace/prices.json").exists());
}

#[test]
fn pricing_is_local_only() {
    let h = Home::new("pricing-remote");
    let r = h.run(&["--remote", "http://127.0.0.1:9", "pricing"]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("not supported for `pricing`"), "{}", r.err);
}
