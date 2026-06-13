//! Load user themes from <config_dir>/themes/*.json. Schema matches the
//! Zig design doc: color values are 256-color palette index strings.

use std::path::Path;

use ratatui::style::Color;
use serde::Deserialize;

use super::Theme;

#[derive(Debug, thiserror::Error)]
pub enum ThemeLoadError {
    #[error("theme io: {0}")]
    Io(#[from] std::io::Error),
    #[error("theme json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid color index {0:?}")]
    BadColor(String),
}

#[derive(Debug, Deserialize)]
struct RawTheme {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    colors: RawColors,
    #[serde(default)]
    icons: RawIcons,
    #[serde(default)]
    spinner: RawSpinner,
}

#[derive(Debug, Default, Deserialize)]
struct RawColors {
    bg_primary: Option<String>,
    bg_secondary: Option<String>,
    bg_input: Option<String>,
    fg_text: Option<String>,
    fg_subtle: Option<String>,
    fg_white: Option<String>,
    accent: Option<String>,
    accent_secondary: Option<String>,
    user: Option<String>,
    assistant: Option<String>,
    system: Option<String>,
    separator: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawIcons {
    logo: Option<String>,
    user: Option<String>,
    assistant: Option<String>,
    system: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawSpinner {
    thinking: Option<Vec<String>>,
    streaming: Option<Vec<String>>,
}

fn color(s: &Option<String>, default: u8) -> Result<Color, ThemeLoadError> {
    match s {
        None => Ok(Color::Indexed(default)),
        Some(v) => v
            .parse::<u8>()
            .map(Color::Indexed)
            .map_err(|_| ThemeLoadError::BadColor(v.clone())),
    }
}

/// Parse one theme JSON. `id` is the file stem.
pub fn parse_theme(id: &str, json: &str) -> Result<Theme, ThemeLoadError> {
    let raw: RawTheme = serde_json::from_str(json)?;
    let c = &raw.colors;
    Ok(Theme {
        id: id.to_string(),
        name: raw.name,
        description: raw.description,
        bg_primary: color(&c.bg_primary, 235)?,
        bg_secondary: color(&c.bg_secondary, 236)?,
        bg_input: color(&c.bg_input, 237)?,
        fg_text: color(&c.fg_text, 252)?,
        fg_subtle: color(&c.fg_subtle, 245)?,
        fg_white: color(&c.fg_white, 255)?,
        accent: color(&c.accent, 141)?,
        accent_secondary: color(&c.accent_secondary, 111)?,
        user: color(&c.user, 114)?,
        assistant: color(&c.assistant, 111)?,
        system: color(&c.system, 221)?,
        separator: color(&c.separator, 141)?,
        icon_logo: raw.icons.logo.unwrap_or_else(|| "⟢".into()),
        icon_user: raw.icons.user.unwrap_or_else(|| "❯".into()),
        icon_assistant: raw.icons.assistant.unwrap_or_else(|| "◈".into()),
        icon_system: raw.icons.system.unwrap_or_else(|| "⚡".into()),
        spinner_thinking: raw
            .spinner
            .thinking
            .unwrap_or_else(|| ["⠋", "⠙", "⠹"].iter().map(|s| s.to_string()).collect()),
        spinner_streaming: raw
            .spinner
            .streaming
            .unwrap_or_else(|| ["◐", "◓", "◑", "◒"].iter().map(|s| s.to_string()).collect()),
    })
}

/// Load all *.json under `dir`. Missing dir -> empty vec. Bad files are
/// skipped (logged via tracing), not fatal.
pub fn load_dir(dir: &Path) -> Vec<Theme> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        match std::fs::read_to_string(&path) {
            Ok(s) => match parse_theme(stem, &s) {
                Ok(t) => out.push(t),
                Err(e) => tracing::warn!("skip theme {}: {e}", path.display()),
            },
            Err(e) => tracing::warn!("skip theme {}: {e}", path.display()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_one_theme_json() {
        let json = r#"{
            "name": "custom",
            "description": "mine",
            "icon": "★",
            "colors": {
                "bg_primary": "16", "bg_secondary": "17", "bg_input": "18",
                "fg_text": "252", "fg_subtle": "245", "fg_white": "255",
                "accent": "200", "accent_secondary": "50",
                "user": "114", "assistant": "111", "system": "221", "separator": "200"
            },
            "icons": {"logo": "L", "user": "U", "assistant": "A", "system": "S"},
            "spinner": {"thinking": ["a","b"], "streaming": ["x"]}
        }"#;
        let t = parse_theme("custom", json).unwrap();
        assert_eq!(t.id, "custom");
        assert_eq!(t.accent, Color::Indexed(200));
        assert_eq!(t.icon_assistant, "A");
        assert_eq!(t.spinner_thinking, vec!["a", "b"]);
    }

    #[test]
    fn invalid_color_index_errors() {
        let json = r#"{"name":"x","colors":{"bg_primary":"notanumber"}}"#;
        assert!(parse_theme("x", json).is_err());
    }

    #[test]
    fn missing_colors_use_defaults() {
        let t = parse_theme("bare", r#"{"name":"bare"}"#).unwrap();
        assert_eq!(t.accent, Color::Indexed(141));
        assert_eq!(t.icon_assistant, "◈");
    }

    #[test]
    fn load_dir_missing_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_dir(&dir.path().join("nope")).is_empty());
    }

    #[test]
    fn load_dir_reads_json_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("neon.json"),
            r#"{"name":"Neon","colors":{"accent":"200"}}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("ignore.txt"), "not a theme").unwrap();
        let themes = load_dir(dir.path());
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].id, "neon");
        assert_eq!(themes[0].accent, Color::Indexed(200));
    }
}
