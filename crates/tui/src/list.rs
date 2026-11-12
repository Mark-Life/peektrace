//! List selection and viewport windowing.

use ratatui::crossterm::event::KeyCode;

/// A selected index over a list; keys move it only while its pane is live.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListSel {
    pub index: usize,
}

impl ListSel {
    /// The index clamped to `count`.
    pub fn at(self, count: usize) -> usize {
        self.index.min(count.saturating_sub(1))
    }

    /// Move for `↑/k`, `↓/j`, `Home`, `End`; true when the key was a move.
    pub fn key(&mut self, code: KeyCode, count: usize) -> bool {
        if count == 0 {
            return false;
        }
        let i = self.at(count);
        self.index = match code {
            KeyCode::Up | KeyCode::Char('k') => i.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => (i + 1).min(count - 1),
            KeyCode::Home => 0,
            KeyCode::End => count - 1,
            _ => return false,
        };
        true
    }

    /// `PgUp` / `PgDn` by `step`.
    pub fn page(&mut self, code: KeyCode, count: usize, step: usize) -> bool {
        let i = self.at(count);
        self.index = match code {
            KeyCode::PageUp => i.saturating_sub(step),
            KeyCode::PageDown => (i + step).min(count.saturating_sub(1)),
            _ => return false,
        };
        true
    }
}

/// `[start, end)` of a `count` list that keeps `index` centred in a
/// `visible`-row viewport.
pub fn window_slice(index: usize, count: usize, visible: usize) -> (usize, usize) {
    if count <= visible {
        return (0, count);
    }
    let start = index.saturating_sub(visible / 2).min(count - visible);
    (start, start + visible)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_keeps_the_selection_centred() {
        assert_eq!(window_slice(0, 3, 10), (0, 3));
        assert_eq!(window_slice(0, 20, 5), (0, 5));
        assert_eq!(window_slice(10, 20, 5), (8, 13));
        assert_eq!(window_slice(19, 20, 5), (15, 20));
    }

    #[test]
    fn keys_clamp() {
        let mut s = ListSel::default();
        assert!(s.key(KeyCode::Up, 3));
        assert_eq!(s.index, 0);
        s.key(KeyCode::End, 3);
        assert_eq!(s.index, 2);
        s.key(KeyCode::Char('j'), 3);
        assert_eq!(s.index, 2);
        assert!(!s.key(KeyCode::Char('x'), 3));
        assert_eq!(s.at(1), 0);
    }
}
