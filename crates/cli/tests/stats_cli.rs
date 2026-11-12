//! `peektrace stats` as a user runs it: a real process over a throwaway home
//! holding one Claude session, where a zsh glob abort is retried with the
//! glob quoted.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

const SLUG: &str = "-home-demo-work";
const SID: &str = "33333333-3333-4333-8333-333333333333";
const GLOBBED: &str = "grep -rn needle src --include=*.ts | head -3";
const QUOTED: &str = "grep -rn needle src --include='*.ts' | head -3";

fn iso(ms: u128) -> String {
    let secs = i64::try_from(ms / 1000).unwrap_or(0);
    let nanos = i32::try_from((ms % 1000) * 1_000_000).unwrap_or(0);
    jiff::Timestamp::new(secs, nanos)
        .map(|t| t.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_default()
}

fn call(at: u128, command: &str, id: &str, output: &str) -> [Value; 2] {
    [
        json!({
            "type": "assistant",
            "requestId": format!("req-{id}"),
            "timestamp": iso(at),
            "message": {
                "model": "claude-opus-4",
                "usage": {
                    "input_tokens": 10,
                    "cache_read_input_tokens": 2000,
                    "cache_creation_input_tokens": 0,
                    "output_tokens": 5,
                },
                "content": [{ "type": "tool_use", "id": id, "name": "Bash", "input": { "command": command } }],
            },
        }),
        json!({
            "type": "user",
            "timestamp": iso(at + 1000),
            "message": { "content": [{ "type": "tool_result", "tool_use_id": id, "content": output }] },
        }),
    ]
}

struct Home {
    root: PathBuf,
    transcript: String,
}

impl Home {
    fn new(tag: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("peektrace-prompt-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join(".claude/projects").join(SLUG);
        fs::create_dir_all(&dir).expect("mkdir");
        let start = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis())
            - 60_000;
        let mut lines = vec![json!({
            "type": "user",
            "cwd": "/home/demo/work",
            "timestamp": iso(start),
            "message": { "content": "find the needle" },
        })];
        lines.extend(call(
            start + 1000,
            GLOBBED,
            "toolu_glob",
            "(eval):1: no matches found: --include=*.ts",
        ));
        lines.extend(call(
            start + 5000,
            QUOTED,
            "toolu_quoted",
            "src/a.ts:3:needle",
        ));
        let transcript = dir.join(format!("{SID}.jsonl"));
        let body: Vec<String> = lines.iter().map(Value::to_string).collect();
        fs::write(&transcript, body.join("\n") + "\n").expect("write");
        Self {
            root,
            transcript: transcript.to_string_lossy().into_owned(),
        }
    }

    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_peektrace"))
            .arg("stats")
            .args(args)
            .env_clear()
            .env("HOME", &self.root)
            .env("CLAUDE_CONFIG_DIR", self.root.join(".claude"))
            .env("CODEX_HOME", self.root.join(".codex"))
            .env("XDG_DATA_HOME", self.root.join(".local/share"))
            .env("PEEKTRACE_DIR", self.root.join(".peektrace"))
            .env("PEEKTRACE_NO_TELEMETRY", "1")
            .current_dir(&self.root)
            .output()
            .expect("run peektrace");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_cause_prompt_names_the_failing_call_by_path_and_id() {
    let h = Home::new("cause");
    let (code, out, err) = h.run(&[
        "prompt",
        "--cause",
        "zsh-glob-flag",
        "--days",
        "7",
        "--agent",
        "claude",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("Goal: "));
    for want in [
        "(last 7 days, claude)".to_owned(),
        "- cause: zsh-glob-flag".to_owned(),
        format!("· toolu_glob · {}", h.transcript),
        format!("  failed: {GLOBBED}"),
        format!("  retry: {QUOTED}"),
        "peektrace stats drill --days 7 --agent claude --cause zsh-glob-flag --json".to_owned(),
    ] {
        assert!(out.contains(&want), "{want}\n{out}");
    }
}

#[test]
fn the_printed_drill_command_finds_the_row_and_its_transcripts() {
    let h = Home::new("drill");
    let (code, out, err) = h.run(&[
        "drill",
        "--days",
        "7",
        "--agent",
        "claude",
        "--cause",
        "zsh-glob-flag",
        "--json",
    ]);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["key"], "cause:zsh-glob-flag");
    assert!(v["hits"].as_array().is_some_and(|h| !h.is_empty()));
    let paths: Vec<&str> = v["paths"]
        .as_object()
        .map(|o| o.values().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    assert!(paths.contains(&h.transcript.as_str()), "{out}");
}

#[test]
fn limit_reaches_rows_past_the_default() {
    let h = Home::new("limit");
    let (code, _, err) = h.run(&[
        "prompt",
        "--cause",
        "zsh-glob-flag",
        "--days",
        "7",
        "--limit",
        "50",
    ]);
    assert_eq!(code, 0, "{err}");
}

#[test]
fn no_flag_prints_the_whole_report_prompt() {
    let h = Home::new("report");
    let (code, out, err) = h.run(&["prompt", "--days", "7"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("  - zsh-glob-flag: 1 hits"), "{out}");
    assert!(out.contains("- full report: peektrace stats --days 7 --json"));
}

#[test]
fn a_key_that_names_no_row_fails() {
    let h = Home::new("nope");
    let (code, _, err) = h.run(&["prompt", "--fail", "nope", "--days", "7"]);
    assert_ne!(code, 0);
    assert!(err.contains("No fail row \"nope\""), "{err}");
}

#[test]
fn two_targets_at_once_is_a_usage_error() {
    let h = Home::new("two");
    let (code, _, err) = h.run(&["prompt", "--cause", "a", "--time", "b"]);
    assert_ne!(code, 0);
    assert!(
        err.contains("at most one of --cause, --fail or --time"),
        "{err}"
    );
}

#[test]
fn flag_errors_match_ts() {
    let h = Home::new("flags");
    let cases: [(&[&str], &str); 5] = [
        (
            &["drill", "--agent", "claude"],
            "Pass exactly one of --cause, --fail, --time, --cluster or --wait with a key from the table.\n",
        ),
        (
            &["prompt", "--cause", "a", "--time", "b"],
            "Pass at most one of --cause, --fail or --time.\n",
        ),
        (
            &["usage", "--tz", "Mars/Base"],
            "Unknown time zone \"Mars/Base\". Pass an IANA name such as UTC or Europe/Berlin.\n",
        ),
        (
            &["usage", "--tz", "UTC", "--days", "0"],
            "--days must be from 1 to 36500, got 0.\n",
        ),
        (&["--days", "abc"], "'abc' is not a integer\n\n"),
    ];
    for (args, err) in cases {
        let (code, out, stderr) = h.run(args);
        assert_eq!(
            (code, out.as_str(), stderr.as_str()),
            (1, "", err),
            "{args:?}"
        );
    }
}

#[test]
fn refresh_counts_and_caches_apart_from_ts() {
    let h = Home::new("refresh");
    let (code, out, _) = h.run(&["refresh", "--agent", "claude", "--json"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("\"transcripts\":") && out.contains("\"cached\": 0"),
        "{out}"
    );
    let (_, again, _) = h.run(&["refresh", "--agent", "claude", "--json"]);
    assert!(again.contains("\"extracted\": 0"), "{again}");
    assert!(h.root.join(".peektrace/stats-rs/index.json").exists());
    assert!(!h.root.join(".peektrace/stats").exists());
}
