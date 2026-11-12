//! Tolerant YAML frontmatter parse and serialize for memory files, with no
//! YAML library: the nested `metadata:` block, the legacy flat form, or none.

use super::types::{Field, Frontmatter, Shape};
use crate::js::json_out;
use crate::js::text::{is_js_whitespace, trim};
use crate::js::value::js_key_order;

const BOM: char = '\u{FEFF}';

/// `s.split(/\r?\n/)`.
pub fn split_lines(s: &str) -> Vec<&str> {
    s.split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

/// Strip one matched pair of surrounding quotes; report whether it was quoted.
fn unquote(v: &str) -> (&str, bool) {
    let t = trim(v);
    let b = t.as_bytes();
    if b.len() >= 2 && b[0] == b[b.len() - 1] && matches!(b[0], b'"' | b'\'') {
        return (&t[1..t.len() - 1], true);
    }
    (t, false)
}

/// A `key: value` line split on the first colon.
fn split_kv(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once(':')?;
    Some((trim(k), v))
}

/// `/^\s+\S/`.
fn indented(line: &str) -> bool {
    let rest = line.trim_start_matches(is_js_whitespace);
    rest.len() < line.len() && !rest.is_empty()
}

/// `/^metadata\s*:\s*$/`.
fn metadata_open(line: &str) -> bool {
    line.strip_prefix("metadata")
        .map(|r| r.trim_start_matches(is_js_whitespace))
        .and_then(|r| r.strip_prefix(':'))
        .is_some_and(|r| r.chars().all(is_js_whitespace))
}

/// `/^metadata:[ \t]+$/`.
fn metadata_trailing_ws(line: &str) -> bool {
    line.strip_prefix("metadata:")
        .is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b == b' ' || b == b'\t'))
}

/// `extra[key] = value`: an assignment to `__proto__` is lost, as in JS.
fn set_extra(extra: &mut Vec<(String, String)>, key: &str, value: &str) {
    if key == "__proto__" {
        return;
    }
    if let Some(slot) = extra.iter_mut().find(|(k, _)| k == key) {
        value.clone_into(&mut slot.1);
    } else {
        extra.push((key.to_owned(), value.to_owned()));
        *extra = js_key_order(std::mem::take(extra));
    }
}

fn nested_field(key: &str) -> Option<Field> {
    match key {
        "node_type" => Some(Field::NodeType),
        "type" => Some(Field::Type),
        "originSessionId" => Some(Field::OriginSessionId),
        "created" => Some(Field::Created),
        "updated" => Some(Field::Updated),
        _ => None,
    }
}

fn top_field(key: &str) -> Option<Field> {
    match key {
        "name" => Some(Field::Name),
        "description" => Some(Field::Description),
        other => nested_field(other),
    }
}

/// A file's frontmatter and body. With no leading `---` fence, or no
/// closing one, the whole text (minus a BOM) is the body.
pub fn parse_frontmatter(file_text: &str) -> (Frontmatter, String) {
    let text = file_text.strip_prefix(BOM).unwrap_or(file_text);
    let lines = split_lines(text);
    let none = || (Frontmatter::default(), text.to_owned());
    if lines.first().map(|l| trim(l)) != Some("---") {
        return none();
    }
    let Some(close) = lines.iter().skip(1).position(|l| trim(l) == "---") else {
        return none();
    };
    let block = &lines[1..=close];
    let mut fm = Frontmatter {
        raw: block.join("\n"),
        shape: Shape::Flat,
        ..Frontmatter::default()
    };
    let body = lines[close + 2..].join("\n");
    let mut in_metadata = false;
    for line in block.iter().filter(|l| !trim(l).is_empty()) {
        if metadata_open(line) {
            fm.shape = Shape::Nested;
            in_metadata = true;
            fm.had_trailing_metadata_ws = metadata_trailing_ws(line);
            continue;
        }
        if in_metadata && indented(line) {
            if let Some((key, value)) = split_kv(line) {
                let (value, _) = unquote(value);
                match nested_field(key) {
                    Some(f) => fm.set(f, value),
                    None => set_extra(&mut fm.extra, key, value),
                }
            }
            continue;
        }
        in_metadata = false;
        let Some((key, value)) = split_kv(line) else {
            continue;
        };
        let (value, quoted) = unquote(value);
        match top_field(key) {
            Some(f) => {
                fm.set(f, value);
                if f == Field::Description {
                    fm.description_quoted = quoted;
                }
            }
            None => set_extra(&mut fm.extra, key, value),
        }
    }
    (fm, body)
}

fn kv_line(out: &mut Vec<String>, key: &str, value: Option<&str>) {
    if let Some(v) = value {
        out.push(format!("{key}: {v}"));
    }
}

/// The block (no fences) in the file's own shape, canonically. Used only
/// when fields change; unchanged frontmatter keeps `raw`.
pub fn serialize_block(fm: &Frontmatter) -> String {
    let mut out = Vec::new();
    kv_line(&mut out, "name", fm.get(Field::Name));
    if let Some(d) = fm.get(Field::Description) {
        let d = if fm.description_quoted {
            let mut q = String::new();
            json_out::push_str(&mut q, d);
            q
        } else {
            d.to_owned()
        };
        out.push(format!("description: {d}"));
    }
    let indent = if fm.shape == Shape::Nested {
        out.push("metadata:".to_owned());
        kv_line(&mut out, "  node_type", fm.get(Field::NodeType));
        "  "
    } else {
        ""
    };
    for (key, f) in [
        ("type", Field::Type),
        ("originSessionId", Field::OriginSessionId),
        ("created", Field::Created),
        ("updated", Field::Updated),
    ] {
        kv_line(&mut out, &format!("{indent}{key}"), fm.get(f));
    }
    out.extend(fm.extra.iter().map(|(k, v)| format!("{k}: {v}")));
    out.join("\n")
}

/// The file text from frontmatter and body.
pub fn compose_file(fm: &Frontmatter, body: &str) -> String {
    if fm.shape == Shape::None {
        body.to_owned()
    } else {
        format!("---\n{}\n---\n{body}", fm.raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_and_flat_shapes() {
        let (fm, body) = parse_frontmatter(
            "---\nname: n\ndescription: \"q d\"\nmetadata:  \n  type: user\n  name: inner\nzeta: z\n---\nbody\n",
        );
        assert_eq!(fm.shape, Shape::Nested);
        assert!(fm.had_trailing_metadata_ws);
        assert!(fm.description_quoted);
        assert_eq!(fm.get(Field::Description), Some("q d"));
        assert_eq!(
            fm.extra,
            [
                ("name".to_owned(), "inner".to_owned()),
                ("zeta".to_owned(), "z".to_owned())
            ]
        );
        assert_eq!(body, "body\n");
        assert_eq!(
            serialize_block(&fm),
            "name: n\ndescription: \"q d\"\nmetadata:\n  type: user\nname: inner\nzeta: z"
        );
    }

    #[test]
    fn field_order_follows_the_file() {
        let (fm, _) =
            parse_frontmatter("---\ntype: a\nname: x\n1: one\n__proto__: p\ntype: b\n---\n");
        assert_eq!(
            fm.fields,
            [(Field::Type, "b".to_owned()), (Field::Name, "x".to_owned())]
        );
        assert_eq!(fm.extra, [("1".to_owned(), "one".to_owned())]);
    }

    #[test]
    fn no_fence_or_unterminated_is_all_body() {
        for t in ["\u{FEFF}plain\r\ntext", "---\nname: x\r\nno close"] {
            let (fm, body) = parse_frontmatter(t);
            assert_eq!(fm, Frontmatter::default());
            assert_eq!(body, t.trim_start_matches('\u{FEFF}'));
        }
    }

    #[test]
    fn metadata_lines() {
        assert!(metadata_open("metadata :  "));
        assert!(!metadata_open(" metadata:"));
        assert!(metadata_trailing_ws("metadata: \t"));
        assert!(!metadata_trailing_ws("metadata :"));
        assert!(!metadata_trailing_ws("metadata:"));
    }
}
