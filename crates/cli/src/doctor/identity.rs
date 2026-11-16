//! Strip the home dir and the user name from the doctor bundle, so a file
//! the user emails does not carry who they are or where their code lives.

use regex::{Captures, Regex};

const HOME_MARK: &str = "~";
const USER_MARK: &str = "<user>";
/// Shorter names match too many ordinary words.
const MIN_USER_LEN: usize = 3;

/// Replaces the home dir with `~` and the user name with `<user>`.
pub struct Identity {
    re: Option<Regex>,
}

/// The form a Claude project slug gives a path: each non-alphanumeric char
/// becomes `-`.
fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect()
}

/// `s` as a case-insensitive pattern, with a word boundary on each side that
/// starts or ends with a word char, so `mark` does not match `markdown`.
fn bounded(s: &str) -> String {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    // ASCII boundary: the build leaves out Unicode `\b`.
    let edge = |w: bool| if w { r"(?-u:\b)" } else { "" };
    format!(
        "{}{}{}",
        edge(word(s.chars().next())),
        regex::escape(s),
        edge(word(s.chars().last()))
    )
}

/// The last path component of `home`, either separator.
fn last_component(home: &str) -> &str {
    home.rsplit(['/', '\\']).next().unwrap_or_default()
}

impl Identity {
    /// `home` is the user's home dir; `names` are `$USER` / `$USERNAME`.
    pub fn new(home: &str, names: &[Option<&str>]) -> Self {
        let home = home.trim_end_matches(['/', '\\']);
        let mut homes: Vec<String> = if home.chars().count() >= 2 {
            vec![home.to_owned(), home.replace('\\', "/")]
        } else {
            Vec::new()
        };
        homes.dedup();
        let mut users: Vec<String> = std::iter::once(Some(last_component(home)))
            .chain(names.iter().copied())
            .flatten()
            .map(str::trim)
            .filter(|n| n.chars().count() >= MIN_USER_LEN)
            .flat_map(|n| [n.to_owned(), slug(n)])
            .collect();
        users.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        users.dedup();
        let group = |name: &str, alts: Vec<String>| {
            (!alts.is_empty()).then(|| format!("(?P<{name}>{})", alts.join("|")))
        };
        let groups: Vec<String> = [
            group("home", homes.iter().map(|h| bounded(h)).collect()),
            group("user", users.iter().map(|u| bounded(u)).collect()),
        ]
        .into_iter()
        .flatten()
        .collect();
        let re = (!groups.is_empty())
            .then(|| Regex::new(&format!("(?i){}", groups.join("|"))).ok())
            .flatten();
        Self { re }
    }

    /// `s` with every home dir and user name replaced, in one pass.
    pub fn scrub(&self, s: &str) -> String {
        self.re.as_ref().map_or_else(
            || s.to_owned(),
            |re| {
                re.replace_all(s, |c: &Captures<'_>| {
                    if c.name("home").is_some() {
                        HOME_MARK
                    } else {
                        USER_MARK
                    }
                })
                .into_owned()
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_home_user_and_slug() {
        let id = Identity::new("/Users/jane.doe/", &[Some("jdoe"), None]);
        let cases = [
            ("/Users/jane.doe/Code/app", "~/Code/app"),
            ("cd /USERS/JANE.DOE && ls", "cd ~ && ls"),
            ("-Users-jane-doe-Code-app", "-Users-<user>-Code-app"),
            ("jane.doe@host", "<user>@host"),
            ("/home/jdoe/x", "/home/<user>/x"),
            ("jdoes and jdoe", "jdoes and <user>"),
            ("at main (/Users/jane.doe/a.rs:1:2)", "at main (~/a.rs:1:2)"),
            ("nothing here", "nothing here"),
        ];
        for (raw, want) in cases {
            assert_eq!(id.scrub(raw), want, "{raw}");
        }
    }

    #[test]
    fn skips_short_names_and_windows_paths() {
        let id = Identity::new(r"C:\Users\al", &[Some("al"), Some("")]);
        assert_eq!(id.scrub(r"C:\Users\al\x"), r"~\x");
        assert_eq!(id.scrub("C:/Users/al/x"), "~/x");
        assert_eq!(id.scrub("al and calendar"), "al and calendar");
        assert_eq!(Identity::new("", &[None]).scrub("/a/b"), "/a/b");
        assert_eq!(Identity::new("/", &[Some("ab")]).scrub("/x"), "/x");
    }
}
