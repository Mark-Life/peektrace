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
    /// The replacement for each `slug{i}` group.
    slug_marks: Vec<String>,
}

/// The form a Claude project slug gives a path: each char outside
/// `[A-Za-z0-9]` becomes `-`.
fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
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

/// The ways a home dir shows up: as is, with `/`, JSON-escaped, and as a
/// project slug paired with the slug's replacement.
fn home_forms(home: &str) -> (Vec<String>, Option<(String, String)>) {
    let home = home.trim_end_matches(['/', '\\']);
    if home.chars().count() < 2 {
        return (Vec::new(), None);
    }
    let forms = vec![
        home.to_owned(),
        home.replace('\\', "/"),
        home.replace('\\', "\\\\"),
    ];
    let parent = &home[..home.len() - last_component(home).len()];
    let slugged = (slug(home), format!("{}{USER_MARK}", slug(parent)));
    (forms, Some(slugged))
}

impl Identity {
    /// `homes` are the user's home dir and its symlink-free path; `names`
    /// are `$USER` / `$USERNAME`.
    pub fn new(homes: &[&str], names: &[Option<&str>]) -> Self {
        let mut forms: Vec<String> = Vec::new();
        let mut slugs: Vec<(String, String)> = Vec::new();
        for home in homes {
            let (f, s) = home_forms(home);
            forms.extend(f);
            slugs.extend(s);
        }
        let by_len = |a: &String, b: &String| b.len().cmp(&a.len()).then_with(|| a.cmp(b));
        forms.sort_by(by_len);
        forms.dedup();
        slugs.sort_by(|a, b| by_len(&a.0, &b.0));
        slugs.dedup_by(|a, b| a.0 == b.0);
        let mut users: Vec<String> = homes
            .iter()
            .map(|h| Some(last_component(h.trim_end_matches(['/', '\\']))))
            .chain(names.iter().copied())
            .flatten()
            .map(str::trim)
            .filter(|n| n.chars().count() >= MIN_USER_LEN)
            .flat_map(|n| [n.to_owned(), slug(n)])
            .collect();
        users.sort_by(by_len);
        users.dedup();
        let group = |name: String, alts: Vec<String>| {
            (!alts.is_empty()).then(|| format!("(?P<{name}>{})", alts.join("|")))
        };
        let groups: Vec<String> = [
            group(
                "home".to_owned(),
                forms.iter().map(|h| bounded(h)).collect(),
            ),
            group(
                "user".to_owned(),
                users.iter().map(|u| bounded(u)).collect(),
            ),
        ]
        .into_iter()
        .chain(
            slugs
                .iter()
                .enumerate()
                .map(|(i, (from, _))| group(format!("slug{i}"), vec![bounded(from)])),
        )
        .flatten()
        .collect();
        let re = (!groups.is_empty())
            .then(|| Regex::new(&format!("(?i){}", groups.join("|"))).ok())
            .flatten();
        let slug_marks = slugs.into_iter().map(|(_, mark)| mark).collect();
        Self { re, slug_marks }
    }

    /// `s` with every home dir and user name replaced, in one pass.
    pub fn scrub(&self, s: &str) -> String {
        self.re.as_ref().map_or_else(
            || s.to_owned(),
            |re| {
                re.replace_all(s, |c: &Captures<'_>| {
                    if c.name("home").is_some() {
                        return HOME_MARK;
                    }
                    if c.name("user").is_some() {
                        return USER_MARK;
                    }
                    self.slug_marks
                        .iter()
                        .enumerate()
                        .find(|(i, _)| c.name(&format!("slug{i}")).is_some())
                        .map_or(USER_MARK, |(_, m)| m.as_str())
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
        let id = Identity::new(&["/Users/jane.doe/"], &[Some("jdoe"), None]);
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
        let id = Identity::new(&[r"C:\Users\al"], &[Some("al"), Some("")]);
        assert_eq!(id.scrub(r"C:\Users\al\x"), r"~\x");
        assert_eq!(id.scrub("C:/Users/al/x"), "~/x");
        assert_eq!(id.scrub("al and calendar"), "al and calendar");
        assert_eq!(Identity::new(&[""], &[None]).scrub("/a/b"), "/a/b");
        assert_eq!(Identity::new(&["/"], &[Some("ab")]).scrub("/x"), "/x");
    }

    #[test]
    fn short_name_slug_escaped_and_real_home() {
        let id = Identity::new(&["/home/al", "/net/srv/export/al2"], &[Some("al")]);
        let cases = [
            (
                "~/.claude/projects/-home-al-app",
                "~/.claude/projects/-home-<user>-app",
            ),
            ("-home-alice", "-home-alice"),
            ("/net/srv/export/al2/x", "~/x"),
            ("-net-srv-export-al2-x", "-net-srv-export-<user>-x"),
        ];
        for (raw, want) in cases {
            assert_eq!(id.scrub(raw), want, "{raw}");
        }
        let win = Identity::new(&[r"C:\Users\al"], &[]);
        assert_eq!(
            win.scrub(r#"{"cwd":"C:\\Users\\al\\x"}"#),
            r#"{"cwd":"~\\x"}"#
        );
        assert_eq!(win.scrub("C--Users-al-x"), "C--Users-<user>-x");
    }

    #[test]
    fn non_ascii_name_matches_its_slug() {
        let id = Identity::new(&["/Users/jos\u{e9}"], &[]);
        assert_eq!(id.scrub("-Users-jos--Code"), "-Users-<user>-Code");
        assert_eq!(id.scrub("/Users/jos\u{e9}/Code"), "~/Code");
    }
}
