//! Parity driver: JSONL `{"code","lang"}` in, one JSON line per input out:
//! `[[[class|null, text], ...], ...]`, same-class neighbours merged.

use std::io::{self, BufRead as _, Write as _};

use peektrace_core::highlight::{Lang, is_highlightable, plain_lines, tokenize_lines};
use peektrace_core::js::json_out::Writer;
use peektrace_core::js::value::JsValue;

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut out = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        let Some(v) = JsValue::parse(&line) else {
            continue;
        };
        let Some(obj) = v.as_obj() else { continue };
        let code = obj.get("code").and_then(JsValue::as_str).unwrap_or("");
        let lang = obj
            .get("lang")
            .and_then(JsValue::as_str)
            .and_then(Lang::parse)
            .unwrap_or(Lang::Text);
        let lines = if is_highlightable(code, lang) {
            tokenize_lines(code, lang)
        } else {
            plain_lines(code)
        };
        let mut w = Writer::new(false);
        w.begin_arr();
        for l in &lines {
            w.elem().begin_arr();
            let mut merged: Vec<(Option<&str>, String)> = Vec::new();
            for t in l {
                let class = t.class.map(peektrace_core::highlight::TokenClass::as_str);
                match merged.last_mut() {
                    Some((c, v)) if *c == class => v.push_str(t.value),
                    _ => merged.push((class, t.value.to_owned())),
                }
            }
            for (class, value) in &merged {
                w.elem().begin_arr();
                match class {
                    Some(c) => w.elem().str(c),
                    None => w.elem().null(),
                }
                w.elem().text(value);
                w.end_arr();
            }
            w.end_arr();
        }
        w.end_arr();
        writeln!(out, "{}", w.finish())?;
    }
    Ok(())
}
