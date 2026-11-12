//! The command line as telemetry may store it: the words a user types for
//! `sessions search` never reach a span. Reads the argv `reorder::normalize`
//! returns, where every query word sits after a `--`.

use crate::reorder::command_path;

/// Index of the `sessions search` word in `argv` (with the program name).
fn free_text_index(argv: &[String]) -> Option<usize> {
    match command_path(argv)? {
        ("sessions", "search", at) => Some(at),
        _ => None,
    }
}

/// True when the command line carries user-typed free text.
pub fn has_free_text(argv: &[String]) -> bool {
    free_text_index(argv).is_some()
}

/// `argv` without the program name, joined by spaces. After `sessions
/// search`, every positional and flag value (and every arg after `--`)
/// becomes `<text>`.
pub fn loggable(argv: &[String]) -> String {
    let words = argv.iter().enumerate().skip(1);
    let Some(cmd) = free_text_index(argv) else {
        return words.map(|(_, a)| a.as_str()).collect::<Vec<_>>().join(" ");
    };
    let sep = argv.iter().position(|a| a == "--");
    words
        .map(|(i, a)| {
            let flag = a.starts_with('-') && sep.is_none_or(|s| i < s);
            if i > cmd && !flag && a != "--" {
                "<text>"
            } else {
                a.as_str()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reorder::normalize;

    fn logged(s: &str) -> String {
        let argv: Vec<String> = s.split(' ').map(str::to_owned).collect();
        loggable(&normalize(&argv))
    }

    #[test]
    fn scrubs_search_text() {
        assert_eq!(
            logged("pt --otel sessions search walrus --limit 3 --json"),
            "--otel sessions search --limit <text> --json -- <text>"
        );
        assert_eq!(
            logged("pt sessions search --limit -1 -- -v x"),
            "sessions search --limit -1 -- <text> <text>"
        );
        assert_eq!(logged("pt sessions ls --limit 3"), "sessions ls --limit 3");
        assert_eq!(logged("pt search x"), "search x");
    }

    #[test]
    fn scrubs_dash_query_words() {
        assert_eq!(
            logged("pt --otel sessions search git push --force -rf"),
            "--otel sessions search -- <text> <text> <text> <text>"
        );
        assert_eq!(
            logged("pt sessions search -rf --json"),
            "sessions search -- <text> <text>"
        );
    }

    #[test]
    fn finds_free_text() {
        let argv = |s: &str| s.split(' ').map(str::to_owned).collect::<Vec<_>>();
        assert!(has_free_text(&argv("pt sessions search x")));
        assert!(has_free_text(&argv(
            "pt --log-level debug sessions search x"
        )));
        assert!(!has_free_text(&argv("pt memory ls")));
        assert!(!has_free_text(&argv("pt search x")));
    }
}
