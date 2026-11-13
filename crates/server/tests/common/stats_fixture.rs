//! The `packages/rpc/test/stats.test.ts` corpus: one Claude session with an
//! aborted separator, a silent piped typecheck, a glob-flag failure and its
//! quoted retry, plus a subagent with two model calls and no tools.

use std::path::{Path, PathBuf};

use peektrace_core::AgentRoots;
use peektrace_core::roots::Source;
use serde_json::{Value, json};

pub const SLUG: &str = "-Users-demo-stats";
pub const SID: &str = "22222222-2222-4222-8222-222222222222";
/// A secret in a command body: no report string may hold it.
pub const SECRET: &str = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";
/// `Date.UTC(2026, 6, 1, 12, 0, 0)`.
pub const FIXED_MS: f64 = 1_782_907_200_000.0;

const ABORTED: &str = "bun test 2>&1 | tail -5; echo ===; ls packages";
const SILENT: &str = "bun run typecheck | tail -3";
const GLOBBED: &str = "grep -rn needle src --include=*.ts | head -3; echo end";
const QUOTED: &str = "grep -rn needle src --include='*.ts' | head -3";

fn usage(input: u64, cache_read: u64, cache_create: u64, output: u64) -> Value {
    json!({
        "input_tokens": input,
        "cache_read_input_tokens": cache_read,
        "cache_creation_input_tokens": cache_create,
        "output_tokens": output,
    })
}

fn tool_use(req: &str, at: &str, usage: &Value, id: &str, command: &str) -> Value {
    json!({
        "type": "assistant",
        "requestId": req,
        "timestamp": at,
        "message": {
            "model": "claude-opus-4",
            "usage": usage,
            "content": [{"type": "tool_use", "id": id, "name": "Bash", "input": {"command": command}}],
        },
    })
}

fn tool_result(at: &str, id: &str, output: &str, is_error: bool) -> Value {
    let mut block = json!({"type": "tool_result", "tool_use_id": id, "content": output});
    if is_error {
        block["is_error"] = json!(true);
    }
    json!({"type": "user", "timestamp": at, "message": {"content": [block]}})
}

fn main_lines() -> Vec<Value> {
    vec![
        json!({
            "type": "user",
            "cwd": "/Users/demo/stats",
            "gitBranch": "main",
            "version": "1.2.3",
            "timestamp": "2026-06-01T10:00:00.000Z",
            "message": {"content": "run the checks"},
        }),
        tool_use(
            "req-1",
            "2026-06-01T10:00:05.000Z",
            &usage(100, 0, 200, 50),
            "tool-1",
            ABORTED,
        ),
        tool_result(
            "2026-06-01T10:00:09.000Z",
            "tool-1",
            "Exit code 1\n(eval):1: = not found",
            true,
        ),
        tool_use(
            "req-2",
            "2026-06-01T10:00:12.000Z",
            &usage(60, 1000, 0, 40),
            "tool-2",
            &format!("EXA_API_KEY={SECRET} {SILENT}"),
        ),
        tool_result(
            "2026-06-01T10:00:20.000Z",
            "tool-2",
            "src/auth.ts(12,3): error TS2339: Property 'x' is missing.",
            false,
        ),
        tool_use(
            "req-3",
            "2026-06-01T10:00:30.000Z",
            &usage(10, 2000, 0, 5),
            "tool-3",
            GLOBBED,
        ),
        tool_result(
            "2026-06-01T10:00:31.000Z",
            "tool-3",
            "(eval):1: no matches found: --include=*.ts\nend",
            false,
        ),
        tool_use(
            "req-4",
            "2026-06-01T10:00:36.000Z",
            &usage(10, 2000, 0, 5),
            "tool-4",
            QUOTED,
        ),
        tool_result(
            "2026-06-01T10:00:37.000Z",
            "tool-4",
            "src/a.ts:3:needle",
            false,
        ),
    ]
}

fn subagent_lines() -> Vec<Value> {
    let call = |req: &str, at: &str, input: u64, cache_read: u64| {
        json!({
            "type": "assistant",
            "isSidechain": true,
            "requestId": req,
            "timestamp": at,
            "message": {
                "model": "claude-haiku",
                "usage": usage(input, cache_read, 0, 50),
                "content": [{"type": "text", "text": "looking"}],
            },
        })
    };
    vec![
        json!({
            "type": "user",
            "isSidechain": true,
            "timestamp": "2026-06-01T10:00:13.000Z",
            "message": {"content": "investigate the typecheck"},
        }),
        call("sa-1", "2026-06-01T10:00:14.000Z", 200, 0),
        call("sa-2", "2026-06-01T10:00:16.000Z", 300, 400),
    ]
}

fn jsonl(lines: &[Value]) -> String {
    let mut out: String = lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    out.push('\n');
    out
}

/// Write the corpus under `root`: `<slug>/<sid>.jsonl`, its subagent, and a
/// `home/CLAUDE.md` rule the glob-flag row matches.
pub fn build(root: &Path) {
    let session = root.join(SLUG);
    std::fs::create_dir_all(session.join(SID).join("subagents")).expect("mkdir");
    std::fs::create_dir_all(root.join("home")).expect("mkdir home");
    std::fs::write(
        root.join("home").join("CLAUDE.md"),
        "# Shell\n- Quote globs passed as flag values: --include='*.ts'.\n",
    )
    .expect("CLAUDE.md");
    std::fs::write(session.join(format!("{SID}.jsonl")), jsonl(&main_lines())).expect("main");
    std::fs::write(
        session.join(SID).join("subagents").join("agent-1.jsonl"),
        jsonl(&subagent_lines()),
    )
    .expect("subagent");
}

/// Claude-only roots over `root`.
pub fn roots(root: &Path) -> AgentRoots {
    AgentRoots {
        claude: vec![Source {
            id: "claude".to_owned(),
            label: "default".to_owned(),
            home: root.join("home").to_string_lossy().into_owned(),
            projects_root: root.to_string_lossy().into_owned(),
        }],
        ..AgentRoots::default()
    }
}

pub fn transcript(root: &Path) -> PathBuf {
    root.join(SLUG).join(format!("{SID}.jsonl"))
}
