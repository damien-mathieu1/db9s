//! Query history per connection, kept in `<config dir>/history.json` so ↑/↓ in the
//! query prompt reach queries from earlier sessions too.
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

/// Queries kept per connection, oldest dropped first.
const KEEP: usize = 100;

/// Connection name -> its queries, oldest first.
pub type History = HashMap<String, Vec<String>>;

fn path() -> Result<PathBuf> {
    let config = crate::config::config_path()?;
    Ok(config.with_file_name("history.json"))
}

/// The saved history; empty if there is none yet or it can't be read.
pub fn load() -> History {
    path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save(history: &History) -> Result<()> {
    let path = path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(history)?)
        .with_context(|| format!("writing {}", path.display()))
}

/// Adds `query` as the newest entry: a repeat moves to the end instead of duplicating.
pub fn push(queries: &mut Vec<String>, query: &str) {
    let query = query.trim();
    if query.is_empty() {
        return;
    }
    queries.retain(|q| q != query);
    queries.push(query.to_string());
    if queries.len() > KEEP {
        queries.drain(..queries.len() - KEEP);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_dedupes_and_caps() {
        let mut q = vec![];
        push(&mut q, " select 1 ");
        push(&mut q, "select 2");
        push(&mut q, "select 1");
        push(&mut q, "   ");
        assert_eq!(q, ["select 2", "select 1"]);
        for i in 0..150 {
            push(&mut q, &format!("q{i}"));
        }
        assert_eq!(q.len(), KEEP);
        assert_eq!(q.last().unwrap(), "q149");
        assert_eq!(q[0], "q50");
    }
}
