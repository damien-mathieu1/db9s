//! Keeps passwords out of config.toml: a connection url can reference environment
//! variables (`${PG_PASSWORD}`) or be the output of a command (`cmd: op read op://db/url`).
//! Resolved only when connecting; the template is what gets stored and displayed.
use anyhow::{anyhow, bail, Context, Result};
use std::process::Stdio;

pub const CMD_PREFIX: &str = "cmd:";

pub async fn resolve(url: &str) -> Result<String> {
    let url = match url.trim().strip_prefix(CMD_PREFIX) {
        Some(command) => run(command.trim()).await?,
        None => url.to_string(),
    };
    expand_env(&url, |name| std::env::var(name).ok())
}

/// Replaces every `${NAME}` with the variable's value; a missing variable is an error
/// rather than an empty string, so a typo never turns into a silently wrong url.
fn expand_env(s: &str, lookup: impl Fn(&str) -> Option<String>) -> Result<String> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| anyhow!("unclosed '${{' in connection url"))?;
        let name = &after[..end];
        let value =
            lookup(name).ok_or_else(|| anyhow!("environment variable {name} is not set"))?;
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Runs `command` through the shell; its trimmed stdout is the url.
async fn run(command: &str) -> Result<String> {
    let (shell, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    // stdin is closed so a command that prompts in the terminal fails instead of
    // fighting the TUI for it (GUI prompts like 1Password's still work)
    let output = tokio::process::Command::new(shell)
        .arg(flag)
        .arg(command)
        .stdin(Stdio::null())
        .output()
        .await
        .with_context(|| format!("running `{command}`"))?;
    if !output.status.success() {
        bail!(
            "`{command}` failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() {
        bail!("`{command}` printed nothing");
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(name: &str) -> Option<String> {
        match name {
            "PASS" => Some("s3cr3t".into()),
            "HOST" => Some("db.internal".into()),
            _ => None,
        }
    }

    #[test]
    fn expands_env_vars() {
        assert_eq!(
            expand_env("postgres://app:${PASS}@${HOST}/db", env).unwrap(),
            "postgres://app:s3cr3t@db.internal/db"
        );
        assert_eq!(
            expand_env("mongodb://localhost", env).unwrap(),
            "mongodb://localhost"
        );
        let err = expand_env("x://${NOPE}", env).unwrap_err().to_string();
        assert!(err.contains("NOPE"), "{err}");
        assert!(expand_env("x://${PASS", env).is_err());
    }

    #[tokio::test]
    async fn runs_commands() {
        assert_eq!(
            resolve("cmd: echo redis://localhost:6379").await.unwrap(),
            "redis://localhost:6379"
        );
        assert!(resolve("cmd: exit 3").await.is_err());
        assert!(resolve("cmd: true").await.is_err());
        assert_eq!(resolve("redis://plain").await.unwrap(), "redis://plain");
    }
}
