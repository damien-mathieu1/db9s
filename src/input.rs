//! Single-line text input with a movable cursor, shared by every text field.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Input {
    value: String,
    /// Position in chars, not bytes, so multi-byte text edits correctly.
    cursor: usize,
}

impl Input {
    /// Cursor starts at the end, ready to append.
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self { value, cursor }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.value
            .char_indices()
            .nth(char_idx)
            .map_or(self.value.len(), |(b, _)| b)
    }

    /// Applies an editing key. Returns false for keys it doesn't use (enter, esc, tab...).
    pub fn handle(&mut self, key: KeyEvent) -> bool {
        let len = self.value.chars().count();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = len,
            KeyCode::Char('u') if ctrl => {
                let b = self.byte_at(self.cursor);
                self.value.drain(..b);
                self.cursor = 0;
            }
            KeyCode::Char(c) if !ctrl => {
                let b = self.byte_at(self.cursor);
                self.value.insert(b, c);
                self.cursor += 1;
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    let b = self.byte_at(self.cursor);
                    self.value.remove(b);
                }
            }
            KeyCode::Delete => {
                if self.cursor < len {
                    let b = self.byte_at(self.cursor);
                    self.value.remove(b);
                }
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(len),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = len,
            _ => return false,
        }
        true
    }

    /// The text, with the cursor drawn as a reversed cell when focused.
    pub fn spans(&self, focused: bool) -> Vec<Span<'static>> {
        if !focused {
            return vec![Span::raw(self.value.clone())];
        }
        let b = self.byte_at(self.cursor);
        let mut rest = self.value[b..].chars();
        let at = rest.next().map_or(" ".to_string(), String::from);
        vec![
            Span::raw(self.value[..b].to_string()),
            Span::styled(at, Style::default().add_modifier(Modifier::REVERSED)),
            Span::raw(rest.as_str().to_string()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(input: &mut Input, code: KeyCode) {
        input.handle(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn edits_at_the_cursor() {
        let mut i = Input::new("héllo");
        press(&mut i, KeyCode::Left);
        press(&mut i, KeyCode::Left);
        press(&mut i, KeyCode::Char('X'));
        assert_eq!(i.value(), "hélXlo");
        press(&mut i, KeyCode::Backspace);
        press(&mut i, KeyCode::Backspace);
        assert_eq!(i.value(), "hélo");
        press(&mut i, KeyCode::Home);
        press(&mut i, KeyCode::Delete);
        assert_eq!(i.value(), "élo");
        press(&mut i, KeyCode::End);
        press(&mut i, KeyCode::Right);
        press(&mut i, KeyCode::Char('!'));
        assert_eq!(i.value(), "élo!");
        assert!(!i.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        assert!(!i.handle(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)));
        assert_eq!(i.value(), "élo!");
    }
}
