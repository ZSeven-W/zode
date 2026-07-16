//! Theme model. Colors are 256-color palette indices (ratatui
//! Color::Indexed), translated from the Zig theme design doc. 11 built-in
//! themes + user themes from ~/.zode/themes/*.json (loader.rs).

use ratatui::style::Color;

pub mod builtin;
pub mod loader;

#[derive(Debug, Clone)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub description: String,
    pub bg_primary: Color,
    pub bg_secondary: Color,
    pub bg_input: Color,
    pub fg_text: Color,
    pub fg_subtle: Color,
    pub fg_white: Color,
    pub accent: Color,
    pub accent_secondary: Color,
    pub user: Color,
    pub assistant: Color,
    pub system: Color,
    pub separator: Color,
    pub icon_logo: String,
    pub icon_user: String,
    pub icon_assistant: String,
    pub icon_system: String,
    pub spinner_thinking: Vec<String>,
    pub spinner_streaming: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ThemeStore {
    themes: Vec<Theme>,
}

pub const FALLBACK_ID: &str = "catppuccin-mocha";

impl ThemeStore {
    pub fn with_builtins() -> Self {
        Self {
            themes: builtin::all(),
        }
    }

    /// Add user themes (loader output); a same-id theme overrides a built-in.
    pub fn merge_user(&mut self, user: Vec<Theme>) {
        for t in user {
            if let Some(slot) = self.themes.iter_mut().find(|x| x.id == t.id) {
                *slot = t;
            } else {
                self.themes.push(t);
            }
        }
    }

    pub fn list(&self) -> &[Theme] {
        &self.themes
    }

    pub fn contains(&self, id: &str) -> bool {
        self.themes.iter().any(|t| t.id == id)
    }

    /// Resolve a theme id, falling back to catppuccin-mocha.
    pub fn resolve(&self, id: Option<&str>) -> Theme {
        id.and_then(|i| self.themes.iter().find(|t| t.id == i))
            .or_else(|| self.themes.iter().find(|t| t.id == FALLBACK_ID))
            .cloned()
            .expect("catppuccin-mocha built-in must exist")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eleven_builtins_present() {
        let store = ThemeStore::with_builtins();
        let ids: Vec<&str> = store.list().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "catppuccin-mocha",
                "aurora-forge",
                "ember-atelier",
                "sakura-paper",
                "arctic-day",
                "lavender-mist",
                "citrus-grove",
                "verdant-signal",
                "cyberpunk",
                "minimal",
                "hacker",
            ]
        );
    }

    #[test]
    fn default_is_catppuccin_mocha() {
        let store = ThemeStore::with_builtins();
        assert_eq!(store.resolve(None).id, "catppuccin-mocha");
        assert_eq!(store.resolve(Some("nonexistent")).id, "catppuccin-mocha");
        assert_eq!(store.resolve(Some("hacker")).id, "hacker");
    }

    #[test]
    fn cyberpunk_accent_is_201() {
        let store = ThemeStore::with_builtins();
        let t = store.resolve(Some("cyberpunk"));
        assert_eq!(t.accent, Color::Indexed(201));
    }

    #[test]
    fn new_theme_palettes_are_available() {
        let store = ThemeStore::with_builtins();

        let aurora = store.resolve(Some("aurora-forge"));
        assert_eq!(aurora.accent, Color::Indexed(80));
        assert_eq!(aurora.accent_secondary, Color::Indexed(141));

        let ember = store.resolve(Some("ember-atelier"));
        assert_eq!(ember.accent, Color::Indexed(215));
        assert_eq!(ember.assistant, Color::Indexed(209));

        let sakura = store.resolve(Some("sakura-paper"));
        assert_eq!(sakura.bg_secondary, Color::Indexed(231));
        assert_eq!(sakura.fg_text, Color::Indexed(236));
        assert_eq!(sakura.accent, Color::Indexed(125));

        let arctic = store.resolve(Some("arctic-day"));
        assert_eq!(arctic.bg_primary, Color::Indexed(255));
        assert_eq!(arctic.accent, Color::Indexed(25));

        let lavender = store.resolve(Some("lavender-mist"));
        assert_eq!(lavender.bg_primary, Color::Indexed(189));
        assert_eq!(lavender.accent, Color::Indexed(61));

        let citrus = store.resolve(Some("citrus-grove"));
        assert_eq!(citrus.bg_primary, Color::Indexed(230));
        assert_eq!(citrus.accent, Color::Indexed(130));

        let verdant = store.resolve(Some("verdant-signal"));
        assert_eq!(verdant.accent, Color::Indexed(114));
        assert_eq!(verdant.system, Color::Indexed(179));
    }

    #[test]
    fn user_theme_overrides_builtin() {
        let mut store = ThemeStore::with_builtins();
        let mut custom = builtin::hacker();
        custom.accent = Color::Indexed(99);
        store.merge_user(vec![custom]);
        assert_eq!(store.resolve(Some("hacker")).accent, Color::Indexed(99));
        // count unchanged (override, not append)
        assert_eq!(store.list().iter().filter(|t| t.id == "hacker").count(), 1);
    }
}
