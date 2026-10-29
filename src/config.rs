use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Backend type. Adding one = a variant here (+ `ALL`/`label`), an arm in
/// `app::connect()`, and a `DataSource` impl under `src/datasource/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Postgres,
    Mongo,
    /// `http(s)://` = management API, `amqp(s)://` = plain AMQP.
    RabbitMq,
    Redis,
    MySql,
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Kind::Postgres,
        Kind::MySql,
        Kind::Mongo,
        Kind::RabbitMq,
        Kind::Redis,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Kind::Postgres => "postgres",
            Kind::Mongo => "mongo",
            Kind::RabbitMq => "rabbitmq",
            Kind::Redis => "redis",
            Kind::MySql => "mysql",
        }
    }

    pub fn next(self) -> Kind {
        let i = Kind::ALL.iter().position(|k| *k == self).unwrap();
        Kind::ALL[(i + 1) % Kind::ALL.len()]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub name: String,
    pub kind: Kind,
    pub url: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_toml_format_is_stable() {
        let cfg: Config = toml::from_str(
            r#"
            editor = "vi"
            [[connections]]
            name = "pg"
            kind = "postgres"
            url = "postgres://localhost/db"
            [[connections]]
            name = "rabbit"
            kind = "rabbitmq"
            url = "amqp://localhost/%2F?queues=q"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.connections[0].kind, Kind::Postgres);
        assert_eq!(cfg.connections[1].kind, Kind::RabbitMq);
        assert_eq!(cfg.connections[1].url, "amqp://localhost/%2F?queues=q");
        let round_trip = toml::to_string(&cfg).unwrap();
        assert!(round_trip.contains("kind = \"rabbitmq\""));
    }

    #[test]
    fn kind_cycles_through_all() {
        let mut k = Kind::ALL[0];
        for _ in 0..Kind::ALL.len() {
            k = k.next();
        }
        assert_eq!(k, Kind::ALL[0]);
    }
}
