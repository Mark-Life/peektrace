//! The command line as telemetry may store it: the words a user types for
//! `sessions search` never reach a span. Reads the argv `reorder::normalize`
//! returns, where every query word sits after a `--`.

use crate::reorder::command_path;

/// Global flags that take a separate value, as TS skips them.
const VALUE_FLAGS: [&str; 3] = ["--remote", "--log-level", "--completions"];

/// Args before a bare `--`; after it every word is a positional.
pub fn before_separator(argv: &[String]) -> &[String] {
    let end = argv.iter().position(|a| a == "--").unwrap_or(argv.len());
    &argv[..end]
}

/// The first positional after the program name, skipping the values of
/// global value flags. Reads the raw command line, as TS does.
fn first_positional(argv: &[String]) -> Option<&str> {
    let mut words = argv.iter().skip(1);
    while let Some(a) = words.next() {
        if VALUE_FLAGS.contains(&a.as_str()) {
            words.next();
        } else if !a.starts_with('-') {
            return Some(a);
        }
    }
    None
}

/// Telemetry records unless `--no-telemetry` comes before any `--`, the
/// command is `statusline` (it runs on every status-bar refresh), or
/// `PEEKTRACE_NO_TELEMETRY` is set to anything, empty included.
pub fn telemetry_on(raw: &[String], opt_out_env: bool) -> bool {
    !(opt_out_env
        || before_separator(raw).iter().any(|a| a == "--no-telemetry")
        || first_positional(raw) == Some("statusline"))
}

/// The `--log-level` value before any `--`, read before clap runs so log
/// lines can print from the start. An unknown value is clap's error later.
pub fn log_level(raw: &[String]) -> Option<crate::otel::LogLevel> {
    let flags = before_separator(raw);
    let value = flags
        .iter()
        .enumerate()
        .find_map(|(i, a)| match a.split_once('=') {
            Some(("--log-level", v)) => Some(v),
            _ if a == "--log-level" => flags.get(i + 1).map(String::as_str),
            _ => None,
        })?;
    clap::ValueEnum::from_str(value, false).ok()
}

/// `-v` read as `--version` before any `--`, as TS rewrites it.
pub fn version_alias(argv: &[String]) -> Vec<String> {
    let end = before_separator(argv).len();
    argv.iter()
        .enumerate()
        .map(|(i, a)| {
            if a == "-v" && i < end {
                "--version".to_owned()
            } else {
                a.clone()
            }
        })
        .collect()
}

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

    fn words(s: &str) -> Vec<String> {
        s.split(' ').map(str::to_owned).collect()
    }

    #[test]
    fn telemetry_opt_outs() {
        let on = |s: &str| telemetry_on(&words(s), false);
        assert!(on("pt sessions ls"));
        assert!(!on("pt --no-telemetry sessions ls"));
        assert!(!on("pt sessions ls --no-telemetry"));
        assert!(!on("pt --no-telemetry --telemetry sessions ls"));
        assert!(on("pt --telemetry sessions ls"));
        assert!(on("pt sessions ls -- --no-telemetry"));
        assert!(on("pt sessions ls --no-telemetry=x"));
        assert!(!on("pt statusline"));
        assert!(!on("pt --remote http://x statusline"));
        assert!(!on("pt --log-level info statusline"));
        assert!(!on("pt --json -- statusline"));
        assert!(on("pt sessions statusline"));
        assert!(!telemetry_on(&words("pt sessions ls"), true));
    }

    #[test]
    fn raw_flags() {
        use crate::otel::LogLevel;
        assert_eq!(
            log_level(&words("pt --log-level info sessions ls")),
            Some(LogLevel::Info)
        );
        assert_eq!(
            log_level(&words("pt sessions ls --log-level=debug")),
            Some(LogLevel::Debug)
        );
        assert_eq!(
            log_level(&words("pt sessions ls -- --log-level debug")),
            None
        );
        assert_eq!(log_level(&words("pt --log-level bogus")), None);
        assert_eq!(
            version_alias(&words("pt -v -- -v")),
            words("pt --version -- -v")
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
