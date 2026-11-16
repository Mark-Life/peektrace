//! Pure edits of MEMORY.md text: build a pointer line and insert, update or
//! remove the line for one file.

use super::parse::normalize_slug;
use crate::js::regex::LazyRe;
use crate::js::text::trim;
use crate::js::units::collapse_ws;
use crate::js_re;

static MD_POINTER: LazyRe = js_re!(r"^\s*[-*]\s+\[[^\]]+\]\(([^)]+)\)", "");
static WIKI_POINTER: LazyRe = js_re!(r"^\s*[-*]\s+\[\[([^\]]+)\]\]", "");

/// `- [Label](file.md) — hook`, without the hook when it is blank.
pub fn index_line_for(label: &str, file_name: &str, hook: Option<&str>) -> String {
    let hook = collapse_ws(hook.unwrap_or(""));
    let base = format!("- [{}]({file_name})", trim(label));
    match trim(&hook) {
        "" => base,
        h => format!("{base} — {h}"),
    }
}

/// Append a pointer line, creating the content when it is blank.
pub fn insert_index_line(raw: &str, line: &str) -> String {
    if trim(raw).is_empty() {
        return format!("{line}\n");
    }
    format!("{}\n{line}\n", raw.trim_end_matches('\n'))
}

/// True when a raw MEMORY.md line points at `slug`.
fn targets_slug(raw: &str, slug: &str) -> bool {
    [&*MD_POINTER, &*WIKI_POINTER]
        .into_iter()
        .find_map(|re| re.captures(raw).and_then(|m| m.get(1)))
        .is_some_and(|t| normalize_slug(t) == slug)
}

/// Drop every pointer line for `slug`.
pub fn remove_index_line(raw: &str, slug: &str) -> String {
    let target = normalize_slug(slug);
    raw.split('\n')
        .filter(|l| !targets_slug(l, &target))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Replace the pointer lines for `slug` with `new_line`, or append it.
pub fn update_index_line(raw: &str, slug: &str, new_line: &str) -> String {
    let target = normalize_slug(slug);
    let lines: Vec<&str> = raw.split('\n').collect();
    if !lines.iter().any(|l| targets_slug(l, &target)) {
        return insert_index_line(raw, new_line);
    }
    lines
        .into_iter()
        .map(|l| {
            if targets_slug(l, &target) {
                new_line
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines() {
        assert_eq!(
            index_line_for(" A ", "a.md", Some(" x \n y ")),
            "- [A](a.md) — x y"
        );
        assert_eq!(index_line_for("A", "a.md", Some("  ")), "- [A](a.md)");
        assert_eq!(insert_index_line(" \n", "- x"), "- x\n");
        assert_eq!(insert_index_line("a\n\n", "- x"), "a\n- x\n");
    }

    #[test]
    fn remove_and_update() {
        let raw = "# T\n- [A](a.md) — h\n* [[A|alias]]\n- [B](sub/b.md#x)\n";
        assert_eq!(remove_index_line(raw, "a"), "# T\n- [B](sub/b.md#x)\n");
        assert_eq!(
            remove_index_line(raw, "B"),
            "# T\n- [A](a.md) — h\n* [[A|alias]]\n"
        );
        assert_eq!(
            update_index_line(raw, "b", "- [B2](b.md)"),
            "# T\n- [A](a.md) — h\n* [[A|alias]]\n- [B2](b.md)\n"
        );
        assert_eq!(
            update_index_line("x", "c", "- [C](c.md)"),
            "x\n- [C](c.md)\n"
        );
    }
}
