//! Parity driver for the TUI's pure view functions: JSONL requests
//! `{"fn": name, ...args}` in, one JSON result per line out.

use std::io::{self, BufRead as _, Write as _};

use peektrace_core::js::json_out::Writer;
use peektrace_core::js::value::JsValue;
use peektrace_tui::fmt::{clip, fmt, fmt_bytes, fmt_k, fmt_pct, fmt_started, sanitize};
use peektrace_tui::list::window_slice;
use peektrace_tui::screens::stats::layout::{columns_of, layout_of, pair_cell};
use peektrace_tui::screens::stats::text::{ago_of, wrap};

fn num(v: &JsValue, k: &str) -> f64 {
    v.get(k).and_then(JsValue::as_num).unwrap_or(0.0)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "small test inputs"
)]
fn int(v: &JsValue, k: &str) -> usize {
    num(v, k) as usize
}

fn text(v: &JsValue, k: &str) -> String {
    v.get(k).and_then(JsValue::as_str).unwrap_or("").to_owned()
}

fn answer(v: &JsValue, w: &mut Writer) {
    match text(v, "fn").as_str() {
        "fmt" => w.text(&fmt(num(v, "n"))),
        "fmtK" => w.text(&fmt_k(num(v, "n"))),
        "fmtPct" => w.text(&fmt_pct(num(v, "n"))),
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "byte sizes"
        )]
        "fmtBytes" => w.text(&fmt_bytes(num(v, "n") as u64)),
        "fmtStarted" => w.text(&fmt_started(v.get("iso").and_then(JsValue::as_str))),
        "sanitize" => w.text(&sanitize(&text(v, "s"))),
        "clip" => w.text(&clip(&text(v, "s"), int(v, "max"))),
        "agoOf" => w.text(&ago_of(num(v, "at"), num(v, "now"))),
        "pairCell" => w.text(&pair_cell(num(v, "runs"), num(v, "sec"), int(v, "width"))),
        "wrap" => w.texts(
            wrap(&text(v, "s"), int(v, "width"))
                .iter()
                .map(String::as_str),
        ),
        "windowSlice" => {
            let (s, e) = window_slice(int(v, "index"), int(v, "count"), int(v, "visible"));
            w.begin_obj();
            w.key("start").int(i64::try_from(s).unwrap_or(0));
            w.key("end").int(i64::try_from(e).unwrap_or(0));
            w.end_obj();
        }
        "layoutOf" => {
            let l = layout_of(u16::try_from(int(v, "height")).unwrap_or(0));
            w.begin_obj();
            for (k, n) in [
                ("failRows", l.fail_rows),
                ("findings", l.findings),
                ("noteLines", l.note_lines),
                ("timeRows", l.time_rows),
            ] {
                w.key(k).int(i64::try_from(n).unwrap_or(0));
            }
            w.end_obj();
        }
        "columnsOf" => {
            let c = columns_of(
                int(v, "content"),
                v.get("measured").is_some_and(JsValue::is_true),
            );
            let i = |n: usize| i64::try_from(n).unwrap_or(0);
            w.begin_obj();
            w.key("wide").bool(c.wide);
            w.key("failWhat").int(i(c.fail.what));
            w.key("failWhere").int(i(c.fail.r#where));
            w.key("timeJob").int(i(c.time.job));
            w.key("timeShare").int(i(c.time.share));
            w.key("timeMedian").int(i(c.time.median));
            w.end_obj();
        }
        _ => w.null(),
    }
}

fn main() -> io::Result<()> {
    let mut out = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let v = JsValue::parse(&line?).unwrap_or(JsValue::Null);
        let mut w = Writer::new(false);
        answer(&v, &mut w);
        writeln!(out, "{}", w.finish())?;
    }
    Ok(())
}
