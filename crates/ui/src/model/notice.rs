//! Task-notification fields: every leaf `<tag>value</tag>` of the body, as
//! the regex `<([a-z][\w-]*)>([^<]*)</\1>` finds them in JS.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeField<'a> {
    pub tag: &'a str,
    pub value: &'a str,
}

impl NoticeField<'_> {
    /// The tag as a label: underscores read as spaces.
    pub fn label(&self) -> String {
        self.tag.replace('_', " ")
    }
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// The leaf at byte `at` (a `<`): `(tag, raw value, end)`.
fn leaf_at(body: &str, at: usize) -> Option<(&str, &str, usize)> {
    let rest = &body.as_bytes()[at + 1..];
    if !rest.first()?.is_ascii_lowercase() {
        return None;
    }
    let name_len = rest.iter().position(|&b| !is_word(b))?;
    let open_end = at + 1 + name_len;
    if body.as_bytes().get(open_end) != Some(&b'>') {
        return None;
    }
    let tag = &body[at + 1..open_end];
    let value_start = open_end + 1;
    let value_len = body[value_start..].find('<')?;
    let value = &body[value_start..value_start + value_len];
    let close = &body[value_start + value_len..];
    let closing = close
        .strip_prefix("</")
        .and_then(|c| c.strip_prefix(tag))
        .is_some_and(|c| c.starts_with('>'));
    closing.then(|| (tag, value, value_start + value_len + tag.len() + 3))
}

/// The non-empty fields of a task notification, in order.
pub fn notification_fields(body: &str) -> Vec<NoticeField<'_>> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(off) = body[at..].find('<') {
        let start = at + off;
        match leaf_at(body, start) {
            Some((tag, value, end)) => {
                let value = peektrace_js::text::trim(value);
                if !value.is_empty() {
                    out.push(NoticeField { tag, value });
                }
                at = end;
            }
            None => at = start + 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(body: &str) -> Vec<(&str, &str)> {
        notification_fields(body)
            .into_iter()
            .map(|f| (f.tag, f.value))
            .collect()
    }

    #[test]
    fn leaves_in_order_blank_ones_dropped() {
        let body = "<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n\
                    <summary> Ran tests </summary>\n<output_file></output_file>\n</task-notification>";
        assert_eq!(
            pairs(body),
            [
                ("task-id", "b1"),
                ("status", "completed"),
                ("summary", "Ran tests")
            ]
        );
    }

    #[test]
    fn mismatched_and_nested_tags_are_not_leaves() {
        assert_eq!(pairs("<a>x</b> <B>y</B> <c><d>z</d></c>"), [("d", "z")]);
        assert_eq!(pairs("plain text, no tags"), []);
        assert_eq!(pairs("<a>unclosed"), []);
    }

    #[test]
    fn label_reads_underscores_as_spaces() {
        let f = NoticeField {
            tag: "tool_use_id",
            value: "x",
        };
        assert_eq!(f.label(), "tool use id");
    }
}
