//! A single-line text editor for the filter, search and settings fields.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Modifier, Style};

use crate::draw::{Seg, seg};
use crate::fmt::{take_width, width};
use crate::theme::{self, fg};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineInput {
    pub text: String,
    /// Cursor as a char index.
    cursor: usize,
}

/// A Ctrl shortcut. Windows reports `AltGr` as Ctrl+Alt, so that pair is
/// typed text, not a shortcut.
pub fn ctrl(k: &KeyEvent) -> bool {
    k.modifiers.contains(KeyModifiers::CONTROL) && !k.modifiers.contains(KeyModifiers::ALT)
}

/// What a key did to the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Changed,
    Moved,
    Submit,
    Cancel,
    Ignored,
}

impl LineInput {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            cursor: text.chars().count(),
        }
    }

    /// Replace the text and put the cursor at its end.
    pub fn set(&mut self, text: &str) {
        *self = Self::new(text);
    }

    fn byte_at(&self, char_index: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_index)
            .map_or(self.text.len(), |(i, _)| i)
    }

    /// Insert text at the cursor; newlines are dropped.
    pub fn insert(&mut self, s: &str) {
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        let at = self.byte_at(self.cursor);
        self.text.insert_str(at, &clean);
        self.cursor += clean.chars().count();
    }

    pub fn key(&mut self, k: KeyEvent) -> Edit {
        let ctrl = ctrl(&k);
        let len = self.text.chars().count();
        match k.code {
            KeyCode::Enter => Edit::Submit,
            KeyCode::Esc => Edit::Cancel,
            KeyCode::Char('u') if ctrl => {
                let at = self.byte_at(self.cursor);
                self.text.drain(..at);
                self.cursor = 0;
                Edit::Changed
            }
            KeyCode::Char('a') if ctrl => {
                self.cursor = 0;
                Edit::Moved
            }
            KeyCode::Char('e') if ctrl => {
                self.cursor = len;
                Edit::Moved
            }
            KeyCode::Char(c) if !ctrl => {
                self.insert(c.encode_utf8(&mut [0; 4]));
                Edit::Changed
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                let at = self.byte_at(self.cursor);
                self.text.remove(at);
                Edit::Changed
            }
            KeyCode::Delete if self.cursor < len => {
                let at = self.byte_at(self.cursor);
                self.text.remove(at);
                Edit::Changed
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                Edit::Moved
            }
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(len);
                Edit::Moved
            }
            KeyCode::Home => {
                self.cursor = 0;
                Edit::Moved
            }
            KeyCode::End => {
                self.cursor = len;
                Edit::Moved
            }
            _ => Edit::Ignored,
        }
    }

    /// The field as segments in `w` columns: the text with a block cursor,
    /// scrolled so the cursor shows, or the placeholder when empty.
    pub fn view(&self, placeholder: &str, w: usize) -> Vec<Seg> {
        let cursor = Style::new().add_modifier(Modifier::REVERSED);
        if self.text.is_empty() {
            let mut ph = placeholder.chars();
            let first = ph.next().map_or_else(|| " ".to_owned(), String::from);
            return vec![
                seg(first, cursor.fg(theme::TEXT_FAINT)),
                seg(ph.collect::<String>(), fg(theme::TEXT_FAINT)),
            ];
        }
        let at = self.byte_at(self.cursor);
        let (before, after) = self.text.split_at(at);
        let mut chars = after.chars();
        let under = chars.next().map_or_else(|| " ".to_owned(), String::from);
        let rest: String = chars.collect();
        let keep = w.saturating_sub(1);
        let shown = if width(before) > keep {
            let skip = width(before) - keep;
            let mut acc = 0;
            let start = before
                .char_indices()
                .find(|(_, c)| {
                    acc += crate::fmt::char_width(*c);
                    acc > skip
                })
                .map_or(before.len(), |(i, c)| i + c.len_utf8());
            &before[start..]
        } else {
            before
        };
        let tail = take_width(&rest, w.saturating_sub(width(shown) + 1)).to_owned();
        vec![
            seg(shown, fg(theme::TEXT)),
            seg(under, cursor.fg(theme::TEXT)),
            seg(tail, fg(theme::TEXT)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(i: &mut LineInput, code: KeyCode) -> Edit {
        i.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn edits_at_the_cursor() {
        let mut i = LineInput::new("ac");
        press(&mut i, KeyCode::Left);
        press(&mut i, KeyCode::Char('b'));
        assert_eq!(i.text, "abc");
        press(&mut i, KeyCode::Backspace);
        assert_eq!(i.text, "ac");
        press(&mut i, KeyCode::End);
        press(&mut i, KeyCode::Char('é'));
        assert_eq!(i.text, "acé");
        assert_eq!(press(&mut i, KeyCode::Enter), Edit::Submit);
        i.insert("x\ny");
        assert_eq!(i.text, "acéxy");
    }

    #[test]
    fn altgr_types_text_and_ctrl_alone_stays_a_shortcut() {
        let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
        let mut i = LineInput::new("C:");
        assert_eq!(
            i.key(KeyEvent::new(KeyCode::Char('\\'), altgr)),
            Edit::Changed
        );
        i.key(KeyEvent::new(KeyCode::Char('u'), altgr));
        assert_eq!(i.text, "C:\\u");
        i.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(i.text, "");
    }
}
