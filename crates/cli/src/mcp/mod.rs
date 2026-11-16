//! `peektrace mcp`: a stdio MCP server with read-only peektrace tools.
//! stdout carries only JSON-RPC lines. Lines are handled concurrently, so
//! replies come back in any order; when stdin closes, calls in flight
//! finish and the process exits.

mod protocol;
mod schema;
mod tools;

use std::io::{BufRead, Write as _};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use peektrace_core::js::value::JsValue;

use self::protocol::{Action, call_reply, handle_line};
use self::tools::{Shared, Tool};
use crate::ls::{Context, UserError};

const MIN_WORKERS: usize = 4;

/// The request lines in `chunk`, split on `\n`, `\r\n` or `\r` only: a raw
/// U+2028 or U+2029 stays inside its line.
fn lines_of(chunk: &[u8]) -> impl Iterator<Item = String> + '_ {
    let body = chunk.strip_suffix(b"\n").unwrap_or(chunk);
    let body = body.strip_suffix(b"\r").unwrap_or(body);
    body.split(|b| *b == b'\r')
        .map(|l| String::from_utf8_lossy(l).into_owned())
}

/// A tool error as the CLI prints it.
fn message(e: &UserError) -> String {
    match e {
        UserError::Validation(m) | UserError::Invalid(m) => m.clone(),
    }
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "Error: Die".to_owned())
}

/// Run a call; a failure or a panic becomes an `isError` result.
fn run_call(tool: &Tool, args: &JsValue, shared: &Shared) -> Result<tools::Answer, String> {
    catch_unwind(AssertUnwindSafe(|| tool.call(args, shared)))
        .map_err(|p| panic_text(p.as_ref()))
        .and_then(|r| r.map_err(|e| message(&e)))
}

fn write_line(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out
        .write_all(line.as_bytes())
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush());
}

struct Job {
    id: JsValue,
    tool: &'static Tool,
    args: JsValue,
}

/// Serve `input` until it ends, then wait for the calls in flight.
pub fn serve(ctx: Context, input: impl BufRead) {
    let shared = Arc::new(Shared::new(ctx));
    let (tx, rx) = mpsc::channel::<Job>();
    let rx = Arc::new(Mutex::new(rx));
    let count = std::thread::available_parallelism()
        .map_or(MIN_WORKERS, usize::from)
        .max(MIN_WORKERS);
    let workers: Vec<_> = (0..count)
        .filter_map(|i| {
            let rx = Arc::clone(&rx);
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name(format!("mcp-{i}"))
                .spawn(move || {
                    loop {
                        let job = rx.lock().ok().and_then(|r| r.recv().ok());
                        let Some(job) = job else { break };
                        let outcome = run_call(job.tool, &job.args, &shared);
                        write_line(&call_reply(&job.id, outcome));
                    }
                })
                .ok()
        })
        .collect();
    read_lines(input, |line| match handle_line(&line) {
        Action::Nothing => {}
        Action::Reply(r) => write_line(&r),
        Action::Call { id, tool, args } => {
            let job = Job { id, tool, args };
            if workers.is_empty() {
                write_line(&call_reply(&job.id, run_call(tool, &job.args, &shared)));
            } else if let Err(mpsc::SendError(job)) = tx.send(job) {
                write_line(&call_reply(&job.id, run_call(tool, &job.args, &shared)));
            }
        }
    });
    drop(tx);
    for w in workers {
        let _ = w.join();
    }
}

/// Hand each request line to `on_line` as it arrives.
fn read_lines(mut input: impl BufRead, mut on_line: impl FnMut(String)) {
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match input.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => lines_of(&buf).for_each(&mut on_line),
        }
    }
}

#[cfg(test)]
mod cache_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// TS `mcp-protocol.test.ts` `requestLines`.
    #[test]
    fn lines_split_on_newlines_only() {
        let mut got = Vec::new();
        read_lines("a\u{2028}b\nc\r\nd\re\nlast".as_bytes(), |l| got.push(l));
        assert_eq!(got, ["a\u{2028}b", "c", "d", "e", "last"]);
        let mut got = Vec::new();
        read_lines("x\u{2029}y\n".as_bytes(), |l| got.push(l));
        assert_eq!(got, ["x\u{2029}y"]);
    }

    /// TS `requestLines` "keeps raw U+2028 and U+2029 inside one request",
    /// with the input cut inside the 3-byte U+2028.
    #[test]
    fn keeps_raw_u2028_and_u2029_across_a_chunk_cut() {
        use std::io::{BufReader, Read as _};
        let line = "{\"k\":\"a\u{2028}b\u{2029}c\"}";
        let bytes = format!("{line}\n{{\"n\":2}}\r\n").into_bytes();
        let cut = bytes.iter().position(|b| *b == 0xe2).map_or(0, |i| i + 1);
        let (a, b) = bytes.split_at(cut);
        let mut got = Vec::new();
        read_lines(BufReader::with_capacity(1, a.chain(b)), |l| got.push(l));
        assert_eq!(got, [line, "{\"n\":2}"]);
        assert!(got[0].contains('\u{2028}'));
    }

    #[test]
    fn panics_become_messages() {
        let p = catch_unwind(|| panic!("boom")).err();
        assert_eq!(p.map(|p| panic_text(p.as_ref())), Some("boom".to_owned()));
    }
}
