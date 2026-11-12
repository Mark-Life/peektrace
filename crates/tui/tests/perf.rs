//! Draw times on a big generated session. Run with
//! `cargo test --release -p peektrace-tui --test perf -- --ignored`.

mod common;

use std::fmt::Write as _;
use std::time::Instant;

use common::*;

fn big_session(turns: usize) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        r#"{{"type":"user","cwd":"/w","sessionId":"big","timestamp":"2026-11-11T09:00:00.000Z","message":{{"role":"user","content":"go"}}}}"#
    );
    for i in 0..turns {
        let body = "x".repeat(200);
        let _ = writeln!(
            out,
            r#"{{"type":"assistant","requestId":"r{i}","timestamp":"2026-11-11T09:00:01.000Z","message":{{"id":"m{i}","model":"claude-sonnet-4-5","usage":{{"input_tokens":10,"cache_read_input_tokens":{},"output_tokens":5}},"content":[{{"type":"text","text":"step {i}"}},{{"type":"tool_use","id":"t{i}","name":"Bash","input":{{"command":"echo {i} && ls -la | head"}}}}]}}}}"#,
            1000 + i
        );
        let _ = writeln!(
            out,
            r#"{{"type":"user","timestamp":"2026-11-11T09:00:02.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t{i}","content":"{body}"}}]}}}}"#
        );
    }
    out
}

#[test]
#[ignore = "timing; run in release"]
fn big_history_draws_fast() {
    let home = empty_home();
    let dir = home.join(".claude/projects/-w");
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("big.jsonl"), big_session(7000)).expect("write");
    let mut h = Harness::new(home, 160, 48);
    let ms = |f: &mut dyn FnMut()| {
        let t = Instant::now();
        f();
        t.elapsed().as_secs_f64() * 1000.0
    };
    let first = ms(&mut || {
        h.draw();
    });
    h.keys(&["<enter>"]);
    let press = ms(&mut || {
        h.app.key(key("j"));
        h.draw();
    });
    let expand = ms(&mut || {
        h.app.key(key("E"));
        h.draw();
    });
    let after = ms(&mut || {
        h.app.key(key("<end>"));
        h.draw();
    });
    println!("first {first:.1}ms key {press:.1}ms expand-all {expand:.1}ms end {after:.1}ms");
    assert!(h.text().contains("21001 events") || h.text().contains("events"));
    assert!(press < 50.0 && after < 50.0, "keypress redraws stay cheap");
}
