//! Color palette and border style ported from mongotui (charmbracelet/lipgloss),
//! so db9s reads as the same "family" of TUI: thick borders, ANSI 256 accents.
use ratatui::style::Color;
use ratatui::widgets::BorderType;

pub const BORDER_FOCUS: Color = Color::Indexed(67); // muted steel blue, toned down from lipgloss "57"
pub const BORDER_BLUR: Color = Color::Indexed(240); // lipgloss "240", muted gray
pub const SELECTED_FG: Color = Color::Indexed(229); // lipgloss "229", pale yellow
pub const SELECTED_BG: Color = BORDER_FOCUS;
pub const DOC_TEXT: Color = Color::Indexed(71); // lipgloss "71", green
pub const MODAL_BORDER: Color = Color::Indexed(6); // lipgloss "6", cyan
pub const ERROR: Color = Color::Indexed(9); // lipgloss "9", red
pub const CONFIRM: Color = Color::Indexed(10); // lipgloss "10", green
pub const HELP_KEY: Color = Color::Indexed(229);
pub const HELP_TEXT: Color = Color::Indexed(240);

pub const BORDER_TYPE: BorderType = BorderType::Thick;
