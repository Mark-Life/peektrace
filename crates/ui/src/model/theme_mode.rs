//! The two-look theme toggle over three stored values: `system` is the
//! absence of an override, not a third look.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

impl Appearance {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    pub const fn of_dark(dark: bool) -> Self {
        if dark { Self::Dark } else { Self::Light }
    }

    /// Capitalised, for the toggle's text.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

/// A stored theme value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// As the inspector's pre-paint script reads it: anything but `light`
    /// or `dark` follows the OS.
    pub fn parse(s: &str) -> Self {
        match s {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }

    /// The pinned look; `None` follows the OS.
    pub const fn dark(self) -> Option<bool> {
        match self {
            Self::System => None,
            Self::Light => Some(false),
            Self::Dark => Some(true),
        }
    }

    pub const fn of_dark(dark: Option<bool>) -> Self {
        match dark {
            None => Self::System,
            Some(false) => Self::Light,
            Some(true) => Self::Dark,
        }
    }
}

/// What a press produces: the look it shows and the value it stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePress {
    pub appearance: Appearance,
    pub store: ThemeMode,
}

/// The next press, from the look on screen. Landing on the OS look stores
/// `system`, so two presses never pin a copy of it. An unknown OS look
/// reads as light.
pub const fn next_theme_press(
    resolved: Option<Appearance>,
    system: Option<Appearance>,
) -> ThemePress {
    let system = match system {
        Some(a) => a,
        None => Appearance::Light,
    };
    let Some(resolved) = resolved else {
        return ThemePress {
            appearance: system,
            store: ThemeMode::System,
        };
    };
    let target = match resolved {
        Appearance::Dark => Appearance::Light,
        Appearance::Light => Appearance::Dark,
    };
    let same = matches!(
        (target, system),
        (Appearance::Light, Appearance::Light) | (Appearance::Dark, Appearance::Dark)
    );
    ThemePress {
        appearance: target,
        store: if same {
            ThemeMode::System
        } else {
            match target {
                Appearance::Light => ThemeMode::Light,
                Appearance::Dark => ThemeMode::Dark,
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::Appearance::{Dark, Light};
    use super::*;

    fn press(r: Option<Appearance>, s: Option<Appearance>) -> (Appearance, ThemeMode) {
        let p = next_theme_press(r, s);
        (p.appearance, p.store)
    }

    #[test]
    fn pins_or_drops_the_override() {
        assert_eq!(press(Some(Light), Some(Light)), (Dark, ThemeMode::Dark));
        assert_eq!(press(Some(Dark), Some(Light)), (Light, ThemeMode::System));
        assert_eq!(press(Some(Dark), Some(Dark)), (Light, ThemeMode::Light));
        assert_eq!(press(Some(Light), Some(Dark)), (Dark, ThemeMode::System));
    }

    #[test]
    fn two_presses_return_to_system() {
        for system in [Light, Dark] {
            let first = next_theme_press(Some(system), Some(system));
            assert_eq!(
                first.store,
                if system == Light {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                }
            );
            assert_eq!(
                next_theme_press(Some(first.appearance), Some(system)).store,
                ThemeMode::System
            );
        }
    }

    #[test]
    fn unknown_looks_fall_back_to_system() {
        assert_eq!(press(None, Some(Dark)), (Dark, ThemeMode::System));
        assert_eq!(press(None, None), (Light, ThemeMode::System));
    }

    #[test]
    fn stored_values_read_like_the_pre_paint_script() {
        assert_eq!(ThemeMode::parse("dark"), ThemeMode::Dark);
        assert_eq!(ThemeMode::parse("light"), ThemeMode::Light);
        assert_eq!(ThemeMode::parse("blue"), ThemeMode::System);
        assert_eq!(ThemeMode::parse("system"), ThemeMode::System);
    }
}
