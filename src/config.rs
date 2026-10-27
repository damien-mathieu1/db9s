use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ConnectionKind {
    Postgres { url: String },
    Mongo { url: String },
    RabbitMq { url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub name: String,
    #[serde(flatten)]
    pub kind: ConnectionKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub connections: Vec<Connection>,
    /// Command used for `e` (row/document edit). Falls back to $EDITOR, then vi.
    #[serde(default)]
    pub editor: Option<String>,
}

pub fn config_path() -> Result<PathBuf> {
    let dirs =
        directories::ProjectDirs::from("", "", "db9s").context("cannot resolve config dir")?;
    Ok(dirs.config_dir().join("config.toml"))
}

pub fn load() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let cfg: Config =
        toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
    Ok(cfg)
}

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = toml::to_string_pretty(cfg)?;
    std::fs::write(&path, raw).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

pub fn add_connection(conn: Connection) -> Result<()> {
    let mut cfg = load().unwrap_or_default();
    cfg.connections.retain(|c| c.name != conn.name);
    cfg.connections.push(conn);
    save(&cfg)
}

pub fn remove_connection(name: &str) -> Result<()> {
    let mut cfg = load().unwrap_or_default();
    cfg.connections.retain(|c| c.name != name);
    save(&cfg)
}

pub fn set_editor(editor: &str) -> Result<()> {
    let mut cfg = load().unwrap_or_default();
    cfg.editor = Some(editor.to_string());
    save(&cfg)
}
