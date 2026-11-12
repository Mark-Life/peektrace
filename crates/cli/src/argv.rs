//! The command line as telemetry may store it: the words a user types for
//! `sessions search` never reach a span.

/// Subcommands whose positional arguments are free text the user typed.
const FREE_TEXT: [&str; 1] = ["search"];

/// Index of a free-text subcommand in `args` (without the program name).
fn free_text_index(args: &[String]) -> Option<usize> {
    let cmd = args.iter().position(|a| FREE_TEXT.contains(&a.as_str()))?;
    (cmd > 0 && args[cmd - 1] == "sessions").then_some(cmd)
}

/// True when the command line carries user-typed free text.
pub fn has_free_text(args: &[String]) -> bool {
    free_text_index(args).is_some()
}

/// `args` joined by spaces, with every positional after a free-text
/// subcommand (and every arg after `--`) replaced by `<text>`. Flag values
/// count as positionals.
pub fn loggable(args: &[String]) -> String {
    let Some(cmd) = free_text_index(args) else {
        return args.join(" ");
    };
    let sep = args.iter().position(|a| a == "--");
    args.iter()
        .enumerate()
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

    fn args(s: &str) -> Vec<String> {
        s.split(' ').map(str::to_owned).collect()
    }

    #[test]
    fn scrubs_search_text() {
        assert_eq!(
            loggable(&args("--otel sessions search walrus --limit 3 --json")),
            "--otel sessions search <text> --limit <text> --json"
        );
        assert_eq!(
            loggable(&args("sessions search --limit -1 -- -v x")),
            "sessions search --limit -1 -- <text> <text>"
        );
        assert_eq!(
            loggable(&args("sessions ls --limit 3")),
            "sessions ls --limit 3"
        );
        assert_eq!(loggable(&args("search x")), "search x");
        assert!(has_free_text(&args("sessions search x")));
        assert!(!has_free_text(&args("memory ls")));
    }
}
