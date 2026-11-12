//! A key press, independent of the terminal or windowing library that produced it.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    /// Any key the app has no binding for (F-keys, page up...), still a key press:
    /// confirm prompts treat it as "no".
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub ctrl: bool,
}

impl KeyEvent {
    pub fn new(code: KeyCode) -> Self {
        Self { code, ctrl: false }
    }

    pub fn ctrl(code: KeyCode) -> Self {
        Self { code, ctrl: true }
    }
}
