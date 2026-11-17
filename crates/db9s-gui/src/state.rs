//! What the GUI remembers between runs, in `<config dir>/gui.json` next to config.toml:
//! dragged column widths, window size, font and zoom.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct GuiState {
    #[serde(default)]
    pub col_widths: HashMap<String, f32>,
    /// Width, height in logical pixels.
    pub window: Option<(f32, f32)>,
    /// Monospace font family; Menlo when unset.
    pub font: Option<String>,
    /// 1.0 = 14px text; Cmd +/- /0.
    pub zoom: Option<f32>,
}

fn path() -> Result<PathBuf> {
    Ok(db9s_core::config::config_path()?.with_file_name("gui.json"))
}

/// The saved state; defaults if there is none yet or it can't be read.
pub fn load() -> GuiState {
    path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save(state: &GuiState) -> Result<()> {
    let path = path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(state)?)
        .with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_default() {
        let s: GuiState = serde_json::from_str(r#"{"zoom": 1.25}"#).unwrap();
        assert_eq!(s.zoom, Some(1.25));
        assert!(s.col_widths.is_empty() && s.window.is_none() && s.font.is_none());
        let back: GuiState = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}
