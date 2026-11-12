//! Argv rewrites so clap reads the command line as `@effect/cli` does:
//! - `sessions search`: a dash word that is not a known flag is query text
//!   (`sessions search git push --force`).
//! - `memory ls|show|rm`: after `--`, dash words past the positional slots
//!   are still flags (`memory ls -- -Users-x --json`).
//!
//! The flag lists mirror `args.rs`; a test checks them against clap.

/// Global flags with no value.
const GLOBAL_SWITCHES: [&str; 9] = [
    "--json",
    "--pretty",
    "--read-only",
    "--otel",
    "--no-telemetry",
    "-h",
    "--help",
    "-v",
    "--version",
];
/// Global flags that take a value.
const GLOBAL_VALUES: [&str; 2] = ["--log-level", "--remote"];
const SEARCH_SWITCHES: [&str; 1] = ["--no-redact"];
const SEARCH_VALUES: [&str; 4] = ["--agent", "--project", "--since", "--limit"];

fn takes_value(word: &str) -> bool {
    GLOBAL_VALUES.contains(&word)
}

/// The leaf subcommand path (up to two words) and the index of its last word.
pub fn command_path(argv: &[String]) -> Option<(&str, &str, usize)> {
    let mut found: Vec<(usize, &str)> = Vec::with_capacity(2);
    let mut i = 1;
    while i < argv.len() && found.len() < 2 {
        let a = argv[i].as_str();
        if a == "--" {
            break;
        }
        if takes_value(a) {
            i += 1;
        } else if !a.starts_with('-') {
            found.push((i, a));
        }
        i += 1;
    }
    match found.as_slice() {
        [(_, group), (at, leaf)] => Some((group, leaf, *at)),
        _ => None,
    }
}

fn is_valued(word: &str) -> bool {
    let name = word.split('=').next().unwrap_or(word);
    SEARCH_VALUES.contains(&name) || GLOBAL_VALUES.contains(&name)
}

fn is_switch(word: &str) -> bool {
    GLOBAL_SWITCHES.contains(&word) || SEARCH_SWITCHES.contains(&word)
}

/// `argv` with the search query words moved after a `--` and known flags
/// (with their values) kept in front. As in `@effect/cli`, flags are read
/// anywhere when the query starts with a plain word; when it starts with an
/// unknown dash word, every word from there on is query text.
fn search(argv: &[String], at: usize) -> Vec<String> {
    let (head, rest) = argv.split_at(at + 1);
    let sep = rest.iter().position(|a| a == "--");
    let (before, forced) = match sep {
        Some(s) => (&rest[..s], &rest[s + 1..]),
        None => (rest, &rest[rest.len()..]),
    };
    let mut flags: Vec<String> = Vec::new();
    let mut query: Vec<String> = Vec::new();
    let mut words = before.iter().peekable();
    while let Some(w) = words.next() {
        let free = query.first().is_some_and(|q| q.starts_with('-'));
        if free {
            query.push(w.clone());
        } else if is_valued(w) && (w.contains('=') || words.peek().is_some()) {
            flags.push(w.clone());
            if !w.contains('=') {
                flags.extend(words.next().cloned());
            }
        } else if is_switch(w) {
            flags.push(w.clone());
        } else {
            query.push(w.clone());
        }
    }
    let tail = (!query.is_empty() || sep.is_some()).then(|| "--".to_owned());
    head.iter()
        .cloned()
        .chain(flags)
        .chain(tail)
        .chain(query)
        .chain(forced.iter().cloned())
        .collect()
}

/// `argv` with the dash words after `--` that the `slots` positionals do
/// not take moved in front of the `--`.
fn memory(argv: &[String], at: usize, slots: usize) -> Vec<String> {
    let Some(sep) = argv.iter().skip(at + 1).position(|a| a == "--") else {
        return argv.to_vec();
    };
    let sep = at + 1 + sep;
    let before = &argv[at + 1..sep];
    let taken = before
        .iter()
        .enumerate()
        .filter(|&(i, a)| !a.starts_with('-') && (i == 0 || !takes_value(&before[i - 1])))
        .count();
    let after = &argv[sep + 1..];
    let (kept, extra) = after.split_at(slots.saturating_sub(taken).min(after.len()));
    let (moved, stray): (Vec<&String>, Vec<&String>) =
        extra.iter().partition(|a| a.starts_with('-'));
    argv[..sep]
        .iter()
        .chain(moved)
        .chain(std::iter::once(&argv[sep]))
        .chain(kept)
        .chain(stray)
        .cloned()
        .collect()
}

/// `argv` (with the program name) rewritten for clap.
pub fn normalize(argv: &[String]) -> Vec<String> {
    match command_path(argv) {
        Some(("sessions", "search", at)) => search(argv, at),
        Some(("memory", "ls", at)) => memory(argv, at, 1),
        Some(("memory", "show" | "rm", at)) => memory(argv, at, 2),
        _ => argv.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn run(s: &str) -> String {
        let argv: Vec<String> = s.split(' ').map(str::to_owned).collect();
        normalize(&argv).join(" ")
    }

    #[test]
    fn search_dash_words_are_query_text() {
        assert_eq!(run("pt sessions search -rf"), "pt sessions search -- -rf");
        assert_eq!(
            run("pt sessions search git push --force"),
            "pt sessions search -- git push --force"
        );
        assert_eq!(
            run("pt sessions search now -rf --json --limit 1"),
            "pt sessions search --json --limit 1 -- now -rf"
        );
        assert_eq!(
            run("pt sessions search walrus --limit"),
            "pt sessions search -- walrus --limit"
        );
        assert_eq!(
            run("pt --pretty sessions search --limit=-1 walrus --agent claude"),
            "pt --pretty sessions search --limit=-1 --agent claude -- walrus"
        );
        assert_eq!(
            run("pt sessions search --log-level debug x --no-redact"),
            "pt sessions search --log-level debug --no-redact -- x"
        );
    }

    #[test]
    fn a_leading_dash_word_makes_the_rest_query_text() {
        assert_eq!(
            run("pt sessions search -rf --json"),
            "pt sessions search -- -rf --json"
        );
        assert_eq!(
            run("pt sessions search --limit 1 --force --json -h"),
            "pt sessions search --limit 1 -- --force --json -h"
        );
    }

    #[test]
    fn search_keeps_the_separator_tail() {
        assert_eq!(
            run("pt sessions search a -- -v --json"),
            "pt sessions search -- a -v --json"
        );
        assert_eq!(
            run("pt sessions search --json -- -v"),
            "pt sessions search --json -- -v"
        );
        assert_eq!(
            run("pt sessions search --json"),
            "pt sessions search --json"
        );
    }

    #[test]
    fn memory_flags_after_the_separator() {
        assert_eq!(
            run("pt memory ls -- -Users-demo-m --json"),
            "pt memory ls --json -- -Users-demo-m"
        );
        assert_eq!(
            run("pt memory show -- -p yaml --json"),
            "pt memory show --json -- -p yaml"
        );
        assert_eq!(
            run("pt memory rm x -- y --read-only"),
            "pt memory rm x --read-only -- y"
        );
        assert_eq!(
            run("pt memory rm --log-level debug x -- y --json"),
            "pt memory rm --log-level debug x --json -- y"
        );
        assert_eq!(run("pt memory ls -- --json"), "pt memory ls -- --json");
        assert_eq!(
            run("pt memory ls -- x y --json"),
            "pt memory ls --json -- x y"
        );
        assert_eq!(run("pt memory ls --json x"), "pt memory ls --json x");
    }

    /// Every named flag of `cmd`, with whether it takes a value.
    fn clap_flags(cmd: &clap::Command) -> BTreeSet<(String, bool)> {
        cmd.get_arguments()
            .filter(|a| !a.is_positional())
            .flat_map(|a| {
                let valued = a.get_action().takes_values();
                a.get_long()
                    .map(|l| format!("--{l}"))
                    .into_iter()
                    .chain(a.get_short().map(|c| format!("-{c}")))
                    .map(move |n| (n, valued))
            })
            .collect()
    }

    fn listed(switches: &[&str], values: &[&str]) -> BTreeSet<(String, bool)> {
        let tag = |v| move |n: &&str| ((*n).to_owned(), v);
        switches
            .iter()
            .map(tag(false))
            .chain(values.iter().map(tag(true)))
            .collect()
    }

    #[test]
    fn flag_lists_match_clap() {
        let mut root = <crate::args::Cli as clap::CommandFactory>::command();
        root.build();
        let globals = listed(&GLOBAL_SWITCHES, &GLOBAL_VALUES);
        assert_eq!(clap_flags(&root), globals);
        let search = root
            .find_subcommand("sessions")
            .and_then(|c| c.find_subcommand("search"))
            .map(clap_flags)
            .unwrap_or_default();
        let own: BTreeSet<_> = search.difference(&globals).cloned().collect();
        assert_eq!(own, listed(&SEARCH_SWITCHES, &SEARCH_VALUES));
    }

    #[test]
    fn other_commands_pass_through() {
        assert_eq!(
            run("pt sessions ls --limit -1"),
            "pt sessions ls --limit -1"
        );
        assert_eq!(run("pt search -rf"), "pt search -rf");
    }
}
