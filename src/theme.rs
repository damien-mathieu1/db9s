//! Themes: a palette plus a border style, read from TOML. Built-ins ship inside the
//! binary (src/themes/*.toml); users add or override them in `<config dir>/themes/<name>.toml`,
//! optionally `inherits = "<other theme>"` so they only redefine what they change.
use anyhow::{anyhow, bail, Context, Result};
use ratatui::style::Color;
use ratatui::widgets::BorderType;
use serde::Deserialize;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{LazyLock, RwLock};

const BUILTIN: [(&str, &str); 6] = [
    ("default", include_str!("themes/default.toml")),
    (
        "catppuccin-mocha",
        include_str!("themes/catppuccin-mocha.toml"),
    ),
    ("dracula", include_str!("themes/dracula.toml")),
    ("gruvbox", include_str!("themes/gruvbox.toml")),
    ("nord", include_str!("themes/nord.toml")),
    ("tokyo-night", include_str!("themes/tokyo-night.toml")),
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub border_focus: Color,
    pub border_blur: Color,
    pub selected_fg: Color,
    pub selected_bg: Color,
    /// Field names in row cards and editors.
    pub field_name: Color,
    pub modal_border: Color,
    pub error: Color,
    pub success: Color,
    /// Keys in the footer hints.
    pub key_hint: Color,
    /// Secondary text: descriptions, hints, connection urls.
    pub muted: Color,
    pub border_type: BorderType,
}

const BLANK: Theme = Theme {
    border_focus: Color::Reset,
    border_blur: Color::Reset,
    selected_fg: Color::Reset,
    selected_bg: Color::Reset,
    field_name: Color::Reset,
    modal_border: Color::Reset,
    error: Color::Reset,
    success: Color::Reset,
    key_hint: Color::Reset,
    muted: Color::Reset,
    border_type: BorderType::Thick,
};

static CURRENT: LazyLock<RwLock<Theme>> =
    LazyLock::new(|| RwLock::new(load("default").expect("built-in default theme is valid")));

/// The active theme; cheap to call (Theme is Copy).
pub fn get() -> Theme {
    *CURRENT.read().unwrap()
}

pub fn set(theme: Theme) {
    *CURRENT.write().unwrap() = theme;
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    inherits: Option<String>,
    border_type: Option<String>,
    #[serde(default)]
    colors: Colors,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Colors {
    border_focus: Option<String>,
    border_blur: Option<String>,
    selected_fg: Option<String>,
    selected_bg: Option<String>,
    field_name: Option<String>,
    modal_border: Option<String>,
    error: Option<String>,
    success: Option<String>,
    key_hint: Option<String>,
    muted: Option<String>,
}

fn user_dir() -> Option<PathBuf> {
    crate::config::config_path()
        .ok()?
        .parent()
        .map(|p| p.join("themes"))
}

/// Built-in names plus every `*.toml` in the user theme dir, sorted.
pub fn names() -> Vec<String> {
    let mut names: Vec<String> = BUILTIN.iter().map(|(n, _)| n.to_string()).collect();
    if let Some(entries) = user_dir().and_then(|d| std::fs::read_dir(d).ok()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "toml") {
                if let Some(stem) = path.file_stem() {
                    names.push(stem.to_string_lossy().into_owned());
                }
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// Loads a theme by name; a user file with the same name as a built-in overrides it.
pub fn load(name: &str) -> Result<Theme> {
    resolve(name, 0)
}

fn resolve(name: &str, depth: usize) -> Result<Theme> {
    if depth > 8 {
        bail!("theme inheritance is too deep (is there a cycle around '{name}'?)");
    }
    let user_file = user_dir().map(|d| d.join(format!("{name}.toml")));
    let (src, builtin) = match user_file.filter(|p| p.exists()) {
        Some(path) => (
            std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?,
            false,
        ),
        None => {
            let (_, src) = BUILTIN.iter().find(|(n, _)| *n == name).ok_or_else(|| {
                let dir = user_dir()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default();
                anyhow!(
                    "unknown theme '{name}' (built-in: {}; or create {dir}/{name}.toml)",
                    BUILTIN.map(|(n, _)| n).join(", ")
                )
            })?;
            (src.to_string(), true)
        }
    };
    let ctx = || format!("theme '{name}'");
    let file: ThemeFile = toml::from_str(&src).with_context(ctx)?;
    let base = match &file.inherits {
        Some(parent) => resolve(parent, depth + 1).with_context(ctx)?,
        // the built-in default defines every key; anything else starts from it
        None if builtin && name == "default" => BLANK,
        None => resolve("default", depth + 1)?,
    };
    apply(base, &file).with_context(ctx)
}

fn apply(mut t: Theme, file: &ThemeFile) -> Result<Theme> {
    if let Some(b) = &file.border_type {
        t.border_type = match b.as_str() {
            "plain" => BorderType::Plain,
            "rounded" => BorderType::Rounded,
            "double" => BorderType::Double,
            "thick" => BorderType::Thick,
            other => bail!("border_type: '{other}' is not one of plain, rounded, double, thick"),
        };
    }
    let c = &file.colors;
    let fields: [(&str, &Option<String>, &mut Color); 10] = [
        ("border_focus", &c.border_focus, &mut t.border_focus),
        ("border_blur", &c.border_blur, &mut t.border_blur),
        ("selected_fg", &c.selected_fg, &mut t.selected_fg),
        ("selected_bg", &c.selected_bg, &mut t.selected_bg),
        ("field_name", &c.field_name, &mut t.field_name),
        ("modal_border", &c.modal_border, &mut t.modal_border),
        ("error", &c.error, &mut t.error),
        ("success", &c.success, &mut t.success),
        ("key_hint", &c.key_hint, &mut t.key_hint),
        ("muted", &c.muted, &mut t.muted),
    ];
    for (key, value, slot) in fields {
        if let Some(v) = value {
            *slot = Color::from_str(v).map_err(|_| {
                anyhow!("colors.{key}: '{v}' is not a color (use \"#rrggbb\", an ANSI index like \"67\", or a name like \"red\")")
            })?;
        }
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Result<Theme> {
        let file: ThemeFile = toml::from_str(src)?;
        let base = match &file.inherits {
            Some(p) => load(p)?,
            None => load("default")?,
        };
        apply(base, &file)
    }

    #[test]
    fn every_builtin_is_complete_and_distinct() {
        let themes: Vec<Theme> = BUILTIN.iter().map(|(n, _)| load(n).unwrap()).collect();
        for (t, (name, _)) in themes.iter().zip(BUILTIN) {
            assert_ne!(t.border_focus, Color::Reset, "{name} misses colors");
            assert_ne!(t.muted, Color::Reset, "{name} misses colors");
        }
        for (i, a) in themes.iter().enumerate() {
            for b in &themes[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn partial_theme_inherits_the_rest() {
        let t = parse("inherits = \"nord\"\n[colors]\nerror = \"red\"").unwrap();
        let nord = load("nord").unwrap();
        assert_eq!(t.error, Color::Red);
        assert_eq!(t.border_focus, nord.border_focus);
        assert_eq!(t.border_type, BorderType::Rounded);

        let t = parse("[colors]\nmuted = \"#010203\"").unwrap();
        assert_eq!(t.muted, Color::Rgb(1, 2, 3));
        assert_eq!(t.error, load("default").unwrap().error);
    }

    #[test]
    fn bad_input_is_reported_with_the_key() {
        let err = parse("[colors]\nerror = \"blu\"").unwrap_err().to_string();
        assert!(err.contains("colors.error"), "{err}");
        let err = parse("border_type = \"wavy\"").unwrap_err().to_string();
        assert!(err.contains("border_type"), "{err}");
        assert!(parse("[colors]\nerorr = \"red\"").is_err());
        assert!(load("does-not-exist").is_err());
    }
}
