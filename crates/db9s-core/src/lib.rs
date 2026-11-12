//! Everything db9s does that isn't drawing: backends, config, secrets, themes and the
//! app state machine. Frontends (the TUI, later a GUI) feed it `key::KeyEvent`s and
//! render `app::App`.
pub mod app;
pub mod config;
pub mod datasource;
pub mod input;
pub mod key;
pub mod secrets;
pub mod theme;
pub mod value;
