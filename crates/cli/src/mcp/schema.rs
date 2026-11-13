//! Tool argument checks, worded as Effect Schema's `TreeFormatter` words a
//! failed decode with `onExcessProperty: "error"`: the first excess key,
//! else the first failing field in schema order.

use peektrace_core::js::json_out::push_str;
use peektrace_core::js::num;
use peektrace_core::js::value::{JsObject, JsValue};

/// What a field holds. Each kind carries the description `tools/list`
/// shows.
#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Str {
        desc: &'static str,
    },
    Bool {
        desc: &'static str,
    },
    /// A literal union, named by its description.
    Lit {
        title: &'static str,
        values: &'static [&'static str],
    },
    /// A safe integer, `> 0` when `positive` else `>= 0`. `desc` replaces
    /// the range message.
    Int {
        positive: bool,
        desc: &'static str,
    },
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Str { .. } => "string",
            Self::Bool { .. } => "boolean",
            Self::Lit { title, .. } => title,
            Self::Int { positive: true, .. } => "int & positive",
            Self::Int {
                positive: false, ..
            } => "int & nonNegative",
        }
    }

    /// The JSON Schema of a field of this kind, keys in the TS order.
    fn json_schema(self) -> JsValue {
        let s = |v: &str| JsValue::Str(v.to_owned());
        let entries: Vec<(&str, JsValue)> = match self {
            Self::Str { desc } => vec![("type", s("string")), ("description", s(desc))],
            Self::Bool { desc } => vec![("type", s("boolean")), ("description", s(desc))],
            Self::Lit { title, values } => vec![
                ("type", s("string")),
                ("enum", JsValue::Arr(values.iter().map(|v| s(v)).collect())),
                ("description", s(title)),
            ],
            Self::Int { positive, desc } => vec![
                ("type", s("integer")),
                ("description", s(desc)),
                (
                    "title",
                    s(if positive { "positive" } else { "nonNegative" }),
                ),
                (
                    if positive {
                        "exclusiveMinimum"
                    } else {
                        "minimum"
                    },
                    JsValue::Num(0.0),
                ),
            ],
        };
        JsValue::Obj(JsObject::from_entries(
            entries.into_iter().map(|(k, v)| (k.to_owned(), v)),
        ))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Field {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
}

/// The `inputSchema` for `fields`: what [`check`] accepts, so the list and
/// the checks cannot drift.
pub fn input_schema(fields: &[Field]) -> JsValue {
    let required = fields
        .iter()
        .filter(|f| f.required)
        .map(|f| JsValue::Str(f.name.to_owned()))
        .collect();
    let properties = fields
        .iter()
        .map(|f| (f.name.to_owned(), f.kind.json_schema()));
    JsValue::Obj(JsObject::from_entries([
        ("type".to_owned(), JsValue::Str("object".to_owned())),
        ("required".to_owned(), JsValue::Arr(required)),
        (
            "properties".to_owned(),
            JsValue::Obj(JsObject::from_entries(properties)),
        ),
        ("additionalProperties".to_owned(), JsValue::Bool(false)),
    ]))
}

/// A line of the error tree and the lines under it.
struct Node {
    label: String,
    children: Vec<Node>,
}

fn leaf(label: impl Into<String>) -> Node {
    Node {
        label: label.into(),
        children: Vec::new(),
    }
}

fn node(label: impl Into<String>, children: Vec<Node>) -> Node {
    Node {
        label: label.into(),
        children,
    }
}

fn draw(n: &Node, indent: &str, out: &mut String) {
    let count = n.children.len();
    for (i, c) in n.children.iter().enumerate() {
        let last = i + 1 == count;
        out.push('\n');
        out.push_str(indent);
        out.push_str(if last { "└─ " } else { "├─ " });
        out.push_str(&c.label);
        draw(
            c,
            &format!("{indent}{}", if last { "   " } else { "│  " }),
            out,
        );
    }
}

fn render(n: &Node) -> String {
    let mut out = n.label.clone();
    draw(n, "", &mut out);
    out
}

/// A value as Effect's `formatUnknown` prints it.
fn fmt(v: &JsValue) -> String {
    match v {
        JsValue::Num(n) => num::to_js_string(*n),
        other => other.stringify(false),
    }
}

fn quoted(s: &str) -> String {
    let mut out = String::new();
    push_str(&mut out, s);
    out
}

/// `{ readonly a?: string | undefined; readonly b: string }`.
pub fn type_expr(fields: &[Field]) -> String {
    let parts: Vec<String> = fields
        .iter()
        .map(|f| {
            if f.required {
                format!("readonly {}: {}", f.name, f.kind.name())
            } else {
                format!("readonly {}?: {} | undefined", f.name, f.kind.name())
            }
        })
        .collect();
    format!("{{ {} }}", parts.join("; "))
}

/// The branch for a present value of the wrong kind, or `None` when it fits.
fn mismatch(kind: Kind, v: &JsValue) -> Option<Node> {
    let actual = fmt(v);
    match kind {
        Kind::Str { .. } => (!matches!(v, JsValue::Str(_)))
            .then(|| leaf(format!("Expected string, actual {actual}"))),
        Kind::Bool { .. } => (!matches!(v, JsValue::Bool(_)))
            .then(|| leaf(format!("Expected boolean, actual {actual}"))),
        Kind::Lit { title, values } => {
            let hit = v.as_str().is_some_and(|s| values.contains(&s));
            (!hit).then(|| {
                let lines = values
                    .iter()
                    .map(|l| leaf(format!("Expected {}, actual {actual}", quoted(l))))
                    .collect();
                node(title, lines)
            })
        }
        Kind::Int { positive, desc } => {
            let refined = |inner: Node| {
                node(
                    "From side refinement failure",
                    vec![node("int", vec![inner])],
                )
            };
            let failure = match v.as_num() {
                None => refined(node(
                    "From side refinement failure",
                    vec![leaf(format!("Expected number, actual {actual}"))],
                )),
                Some(n) if !num::is_safe_integer(n) => refined(node(
                    "Predicate refinement failure",
                    vec![leaf(format!("Expected an integer, actual {actual}"))],
                )),
                Some(n) if (positive && n <= 0.0) || n < 0.0 => node(
                    "Predicate refinement failure",
                    vec![leaf(format!("Expected {desc}, actual {actual}"))],
                ),
                Some(_) => return None,
            };
            Some(node(kind.name(), vec![failure]))
        }
    }
}

/// The error under a field's key, or `None` when the field passes.
fn field_error(f: &Field, v: Option<&JsValue>) -> Option<Vec<Node>> {
    match (v, f.required) {
        (None, true) => Some(vec![leaf("is missing")]),
        (None, false) => None,
        (Some(v), true) => mismatch(f.kind, v).map(|m| vec![m]),
        (Some(v), false) => mismatch(f.kind, v).map(|m| {
            vec![node(
                format!("{} | undefined", f.kind.name()),
                vec![m, leaf(format!("Expected undefined, actual {}", fmt(v)))],
            )]
        }),
    }
}

/// The arguments when they fit `fields`, else the error text after
/// `Invalid arguments for <tool>: `.
pub fn check<'a>(fields: &[Field], args: &'a JsValue) -> Result<&'a JsObject, String> {
    let expr = type_expr(fields);
    let Some(obj) = args.as_obj() else {
        return Err(format!("Expected {expr}, actual {}", fmt(args)));
    };
    let key = |k: &str| format!("[{}]", quoted(k));
    let excess = obj
        .keys()
        .find(|k| !fields.iter().any(|f| f.name == *k))
        .map(|k| {
            let names: Vec<String> = fields.iter().map(|f| quoted(f.name)).collect();
            node(
                key(k),
                vec![leaf(format!(
                    "is unexpected, expected: {}",
                    names.join(" | ")
                ))],
            )
        });
    let failed = excess.or_else(|| {
        fields.iter().find_map(|f| {
            field_error(f, obj.get(f.name)).map(|children| node(key(f.name), children))
        })
    });
    match failed {
        Some(n) => Err(render(&node(expr, vec![n]))),
        None => Ok(obj),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGENT: Kind = Kind::Lit {
        title: "Agent to scope to (default all)",
        values: &["claude", "codex", "pi", "opencode", "all"],
    };
    const FIELDS: [Field; 4] = [
        Field {
            name: "q",
            kind: Kind::Str { desc: "" },
            required: true,
        },
        Field {
            name: "agent",
            kind: AGENT,
            required: false,
        },
        Field {
            name: "latest",
            kind: Kind::Bool { desc: "" },
            required: false,
        },
        Field {
            name: "limit",
            kind: Kind::Int {
                positive: false,
                desc: "Max sessions",
            },
            required: false,
        },
    ];
    const EXPR: &str = "{ readonly q: string; readonly agent?: Agent to scope to (default all) | undefined; readonly latest?: boolean | undefined; readonly limit?: int & nonNegative | undefined }";

    fn err_of(args: &JsValue) -> String {
        check(&FIELDS, args).err().unwrap_or_default()
    }

    fn err(args: &str) -> String {
        err_of(&JsValue::parse(args).unwrap_or(JsValue::Null))
    }

    /// Every listed property passes the excess-key check, and only the
    /// required fields are listed as required.
    #[test]
    fn schema_lists_what_check_accepts() {
        let schema = input_schema(&FIELDS);
        let props = schema.get("properties").and_then(JsValue::as_obj);
        let keys: Vec<&str> = props.map(|p| p.keys().collect()).unwrap_or_default();
        assert_eq!(keys, FIELDS.map(|f| f.name));
        for k in keys {
            let args = JsValue::parse(&format!(r#"{{"{k}":null}}"#)).unwrap_or(JsValue::Null);
            assert!(!err_of(&args).contains("is unexpected"), "{k}");
        }
        assert_eq!(
            schema
                .get("required")
                .map(|r| r.stringify(false))
                .as_deref(),
            Some(r#"["q"]"#)
        );
    }

    #[test]
    fn passes() {
        let v = JsValue::parse(r#"{"q":"x","limit":-0,"agent":"pi"}"#).unwrap_or(JsValue::Null);
        assert!(check(&FIELDS, &v).is_ok());
    }

    #[test]
    fn not_an_object() {
        assert_eq!(err("[]"), format!("Expected {EXPR}, actual []"));
        assert_eq!(err(r#""x""#), format!("Expected {EXPR}, actual \"x\""));
    }

    #[test]
    fn excess_first() {
        assert_eq!(
            err(r#"{"redact":false,"limit":-1}"#),
            format!(
                "{EXPR}\n└─ [\"redact\"]\n   └─ is unexpected, expected: \"q\" | \"agent\" | \"latest\" | \"limit\""
            )
        );
    }

    #[test]
    fn fields_in_order() {
        assert_eq!(err("{}"), format!("{EXPR}\n└─ [\"q\"]\n   └─ is missing"));
        assert_eq!(
            err(r#"{"q":5}"#),
            format!("{EXPR}\n└─ [\"q\"]\n   └─ Expected string, actual 5")
        );
        assert_eq!(
            err(r#"{"q":"x","latest":"yes"}"#),
            format!(
                "{EXPR}\n└─ [\"latest\"]\n   └─ boolean | undefined\n      ├─ Expected boolean, actual \"yes\"\n      └─ Expected undefined, actual \"yes\""
            )
        );
        assert_eq!(
            err(r#"{"q":"x","limit":-1,"agent":"x"}"#),
            format!(
                "{EXPR}\n└─ [\"agent\"]\n   └─ Agent to scope to (default all) | undefined\n      ├─ Agent to scope to (default all)\n      │  ├─ Expected \"claude\", actual \"x\"\n      │  ├─ Expected \"codex\", actual \"x\"\n      │  ├─ Expected \"pi\", actual \"x\"\n      │  ├─ Expected \"opencode\", actual \"x\"\n      │  └─ Expected \"all\", actual \"x\"\n      └─ Expected undefined, actual \"x\""
            )
        );
    }

    #[test]
    fn integers() {
        let int = |v: &str, body: &str| {
            assert_eq!(
                err(&format!(r#"{{"q":"x","limit":{v}}}"#)),
                format!(
                    "{EXPR}\n└─ [\"limit\"]\n   └─ int & nonNegative | undefined\n      ├─ int & nonNegative\n{body}\n      └─ Expected undefined, actual {v}"
                )
            );
        };
        int(
            "\"5\"",
            "      │  └─ From side refinement failure\n      │     └─ int\n      │        └─ From side refinement failure\n      │           └─ Expected number, actual \"5\"",
        );
        int(
            "1.5",
            "      │  └─ From side refinement failure\n      │     └─ int\n      │        └─ Predicate refinement failure\n      │           └─ Expected an integer, actual 1.5",
        );
        int(
            "-1",
            "      │  └─ Predicate refinement failure\n      │     └─ Expected Max sessions, actual -1",
        );
    }
}
