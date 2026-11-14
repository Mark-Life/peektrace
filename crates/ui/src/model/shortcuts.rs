//! Keyboard shortcuts: one table for the handlers and the help dialog,
//! keycap labels, and the layout-aware key reading.

use peektrace_js::units;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    General,
    Sessions,
    Transcript,
}

impl Group {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Sessions => "Sessions",
            Self::Transcript => "Transcript",
        }
    }
}

/// Who handles a key: `Global` the window, `List` the session rail while a
/// row has focus, `Builtin` a widget (documented only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Builtin,
    Global,
    List,
}

impl Scope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Global => "global",
            Self::List => "list",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub id: &'static str,
    /// `KeyboardEvent.key` names, as [`key_of`] reads them; `Mod+` is ⌘ on
    /// a Mac and Ctrl elsewhere.
    pub keys: &'static [&'static str],
    pub label: &'static str,
    pub group: Group,
    pub scope: Scope,
}

const fn sc(
    id: &'static str,
    keys: &'static [&'static str],
    label: &'static str,
    group: Group,
    scope: Scope,
) -> Shortcut {
    Shortcut {
        id,
        keys,
        label,
        group,
        scope,
    }
}

pub const SHORTCUTS: [Shortcut; 11] = [
    sc(
        "listDown",
        &["j", "ArrowDown"],
        "Next session",
        Group::Sessions,
        Scope::List,
    ),
    sc(
        "listUp",
        &["k", "ArrowUp"],
        "Previous session",
        Group::Sessions,
        Scope::List,
    ),
    sc(
        "listFirst",
        &["Home"],
        "First session",
        Group::Sessions,
        Scope::List,
    ),
    sc(
        "listLast",
        &["End"],
        "Last session",
        Group::Sessions,
        Scope::List,
    ),
    sc(
        "listOpen",
        &["Enter"],
        "Open the focused session",
        Group::Sessions,
        Scope::Builtin,
    ),
    sc(
        "search",
        &["/"],
        "Focus search",
        Group::Sessions,
        Scope::Global,
    ),
    sc(
        "prevTurn",
        &["["],
        "Previous turn",
        Group::Transcript,
        Scope::Global,
    ),
    sc(
        "nextTurn",
        &["]"],
        "Next turn",
        Group::Transcript,
        Scope::Global,
    ),
    sc(
        "theme",
        &["d"],
        "Switch light / dark theme",
        Group::General,
        Scope::Global,
    ),
    sc(
        "sidebar",
        &["Mod+b"],
        "Collapse or expand the sidebar",
        Group::General,
        Scope::Builtin,
    ),
    sc(
        "help",
        &["?"],
        "Show keyboard shortcuts",
        Group::General,
        Scope::Global,
    ),
];

/// Help dialog sections, in order.
pub const GROUPS: [Group; 3] = [Group::Sessions, Group::Transcript, Group::General];

pub const HELP_TITLE: &str = "Keyboard shortcuts";
pub const HELP_NOTE: &str = "Single keys do nothing while you type in a text field.";
pub const HELP_BUTTON: &str = "Shortcuts";

/// Where a group's keys work, when narrower than the whole app.
pub const fn group_note(g: Group) -> Option<&'static str> {
    match g {
        Group::Sessions => Some("Moving and opening work while a session row has focus."),
        Group::Transcript => {
            Some("In an open transcript. The table view moves by turn only when sorted by Turn.")
        }
        Group::General => None,
    }
}

pub fn shortcut(id: &str) -> Option<&'static Shortcut> {
    SHORTCUTS.iter().find(|s| s.id == id)
}

/// A single character some shortcut is bound to.
pub fn bound(key: &str) -> bool {
    units::len(key) == 1 && SHORTCUTS.iter().any(|s| s.keys.contains(&key))
}

/// How a key reads on a keycap: `Mod+b` as `⌘B` on a Mac, `Ctrl+B` elsewhere.
pub fn key_label(key: &str, mac: bool) -> String {
    let (modded, base) = key.strip_prefix("Mod+").map_or((false, key), |b| (true, b));
    let shown = match base {
        "ArrowDown" => "↓".to_owned(),
        "ArrowUp" => "↑".to_owned(),
        b if units::len(b) == 1 => b.to_uppercase(),
        b => b.to_owned(),
    };
    match (modded, mac) {
        (false, _) => shown,
        (true, true) => format!("⌘{shown}"),
        (true, false) => format!("Ctrl+{shown}"),
    }
}

/// Physical keys read when the layout types a character no shortcut binds.
fn code_key(code: &str) -> Option<(&'static str, &'static str)> {
    match code {
        "BracketLeft" => Some(("[", "{")),
        "BracketRight" => Some(("]", "}")),
        "Slash" => Some(("/", "?")),
        _ => None,
    }
}

/// The key as the table names it (`keyOf`). Single letters lowercase. A
/// symbol no shortcut binds (`.` from the Slash key on a Russian layout)
/// falls back to the physical key `code` (a DOM `KeyboardEvent.code`), and
/// so does a non-Latin letter. A Latin letter or digit never does.
pub fn key_of(key: &str, code: &str, shift: bool) -> String {
    if units::len(key) != 1 {
        return key.to_owned();
    }
    let typed = key
        .chars()
        .next()
        .filter(|c| (' '..='~').contains(c))
        .map(|c| c.to_ascii_lowercase().to_string());
    if let Some(t) = &typed
        && (bound(t)
            || t.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
    {
        return t.clone();
    }
    if let Some((plain, shifted)) = code_key(code) {
        return if shift { shifted } else { plain }.to_owned();
    }
    if let Some(t) = typed {
        return t;
    }
    if let Some(letter) = code.strip_prefix("Key")
        && units::len(code) == 4
    {
        return letter.to_lowercase();
    }
    key.to_owned()
}

/// The modifiers a key event carries, as the browser names them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[expect(clippy::struct_excessive_bools, reason = "independent modifier keys")]
pub struct Modifiers {
    pub meta: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub alt_graph: bool,
}

/// Whether a plain-key shortcut may run with these modifiers. On a Mac,
/// Option types brackets on many layouts, so Option is allowed when it
/// typed a bound key.
pub fn shortcut_allowed(m: Modifiers, key: &str, mac: bool) -> bool {
    let option = mac && m.alt && !m.ctrl && bound(key);
    !(m.meta || ((m.ctrl || m.alt) && !(m.alt_graph || option)))
}

/// Whether `key` (as [`key_of`] reads it) presses shortcut `id`.
pub fn matches(id: &str, key: &str) -> bool {
    shortcut(id).is_some_and(|s| s.keys.contains(&key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_keys_read_as_typed() {
        assert_eq!(key_of("j", "KeyJ", false), "j");
        assert_eq!(key_of("D", "KeyD", true), "d");
        assert_eq!(key_of("ArrowDown", "ArrowDown", false), "ArrowDown");
    }

    #[test]
    fn cyrillic_falls_back_to_the_physical_key() {
        assert_eq!(key_of("о", "KeyJ", false), "j");
        assert_eq!(key_of("в", "KeyD", false), "d");
        assert_eq!(key_of("х", "BracketLeft", false), "[");
        assert_eq!(key_of("ъ", "BracketRight", false), "]");
    }

    #[test]
    fn question_mark_is_the_shifted_slash() {
        assert_eq!(key_of("?", "Slash", true), "?");
        assert!(matches("help", &key_of("?", "Slash", false)));
        assert!(matches("search", &key_of("/", "Slash", false)));
    }

    #[test]
    fn other_layouts() {
        assert_eq!(key_of(".", "Slash", false), "/");
        assert_eq!(key_of(",", "Slash", true), "?");
        assert_eq!(key_of("+", "BracketRight", false), "]");
        assert_eq!(key_of("]", "Digit6", false), "]");
        assert_eq!(key_of("/", "Digit7", true), "/");
        assert_eq!(key_of("z", "Slash", false), "z");
        assert_eq!(key_of("Z", "Slash", true), "z");
        assert!(!matches("search", &key_of("z", "Slash", false)));
        assert!(!matches("help", &key_of("Z", "Slash", false)));
        assert_eq!(key_of(".", "Period", false), ".");
        assert_eq!(key_of("😀", "KeyA", false), "😀");
    }

    #[test]
    fn modifier_rules() {
        let none = Modifiers::default();
        assert!(shortcut_allowed(none, "j", false));
        assert!(
            shortcut_allowed(none, "?", false),
            "Shift alone is no chord"
        );
        let meta = Modifiers { meta: true, ..none };
        let ctrl = Modifiers { ctrl: true, ..none };
        let alt = Modifiers { alt: true, ..none };
        assert!(!shortcut_allowed(meta, "j", false));
        assert!(!shortcut_allowed(ctrl, "j", false));
        assert!(!shortcut_allowed(alt, "j", false));
        let alt_gr = Modifiers {
            alt: true,
            ctrl: true,
            alt_graph: true,
            meta: false,
        };
        assert!(shortcut_allowed(alt_gr, "[", false));
        assert!(shortcut_allowed(alt, "]", true));
        assert!(shortcut_allowed(alt, "[", true));
        assert!(!shortcut_allowed(alt, "∂", true));
        assert!(!shortcut_allowed(alt, "]", false));
        assert!(!shortcut_allowed(
            Modifiers { ctrl: true, ..alt },
            "]",
            true
        ));
    }

    #[test]
    fn labels() {
        assert_eq!(key_label("Mod+b", true), "⌘B");
        assert_eq!(key_label("Mod+b", false), "Ctrl+B");
        assert_eq!(key_label("ArrowUp", false), "↑");
        assert_eq!(key_label("d", false), "D");
        assert_eq!(key_label("Home", false), "Home");
    }

    #[test]
    fn ids_are_unique_and_groups_cover_the_table() {
        let mut ids: Vec<&str> = SHORTCUTS.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), SHORTCUTS.len());
        assert!(SHORTCUTS.iter().all(|s| GROUPS.contains(&s.group)));
    }
}
