//! Runtime i18n for the Zode TUI. Covers the 15 languages of the OpenPencil
//! ecosystem (en, zh, zh-tw, ja, ko, es, fr, de, pt, ru, hi, id, th, tr, vi).
//!
//! Design: the English string IS the key. [`t`] wraps a string literal and
//! returns its translation for the currently-selected language, falling back
//! to the English literal when there's no translation. So wiring a UI string
//! for i18n is just `t("Settings")` instead of `"Settings"`. The current
//! language lives in a process-global so a `/language` (Settings) switch takes
//! effect on the next render without threading state through every widget.
//!
//! Translation data lives in the generated [`i18n_data`] module
//! (`SOURCE_STRINGS` ⇄ `LOCALES`, aligned by index).

use std::sync::RwLock;

use crate::i18n_data::{LOCALES, SOURCE_STRINGS};

/// The 15 supported UI languages (OpenPencil set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Zh,
    ZhTw,
    Ja,
    Ko,
    Es,
    Fr,
    De,
    Pt,
    Ru,
    Hi,
    Id,
    Th,
    Tr,
    Vi,
}

impl Lang {
    /// Every language, in the canonical order (English first).
    pub const ALL: [Lang; 15] = [
        Lang::En,
        Lang::Zh,
        Lang::ZhTw,
        Lang::Ja,
        Lang::Ko,
        Lang::Es,
        Lang::Fr,
        Lang::De,
        Lang::Pt,
        Lang::Ru,
        Lang::Hi,
        Lang::Id,
        Lang::Th,
        Lang::Tr,
        Lang::Vi,
    ];

    /// BCP-47-ish code used in config (`language: "zh-tw"`).
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Zh => "zh",
            Lang::ZhTw => "zh-tw",
            Lang::Ja => "ja",
            Lang::Ko => "ko",
            Lang::Es => "es",
            Lang::Fr => "fr",
            Lang::De => "de",
            Lang::Pt => "pt",
            Lang::Ru => "ru",
            Lang::Hi => "hi",
            Lang::Id => "id",
            Lang::Th => "th",
            Lang::Tr => "tr",
            Lang::Vi => "vi",
        }
    }

    /// Endonym for the language picker (shown in the language's own script).
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Zh => "简体中文",
            Lang::ZhTw => "繁體中文",
            Lang::Ja => "日本語",
            Lang::Ko => "한국어",
            Lang::Es => "Español",
            Lang::Fr => "Français",
            Lang::De => "Deutsch",
            Lang::Pt => "Português",
            Lang::Ru => "Русский",
            Lang::Hi => "हिन्दी",
            Lang::Id => "Bahasa Indonesia",
            Lang::Th => "ไทย",
            Lang::Tr => "Türkçe",
            Lang::Vi => "Tiếng Việt",
        }
    }

    /// Parse a config code (case-insensitive; `zh_TW`/`zh-tw` both work).
    pub fn from_code(code: &str) -> Option<Lang> {
        let c = code.trim().to_ascii_lowercase().replace('_', "-");
        Lang::ALL.into_iter().find(|l| l.code() == c)
    }

    /// Row index into `LOCALES` (the 14 non-English languages). `None` for En,
    /// which needs no lookup (it's the source).
    fn locale_row(self) -> Option<usize> {
        match self {
            Lang::En => None,
            // Order matches LOCALES rows below.
            other => Lang::ALL.iter().skip(1).position(|l| *l == other),
        }
    }
}

static CURRENT: RwLock<Lang> = RwLock::new(Lang::En);

/// Set the active UI language (from the Settings language picker / config).
pub fn set_language(lang: Lang) {
    if let Ok(mut g) = CURRENT.write() {
        *g = lang;
    }
}

/// Set the active language by code (e.g. "zh-tw"). Returns false for an
/// unknown code (the language is left unchanged).
pub fn set_language_code(code: &str) -> bool {
    match Lang::from_code(code) {
        Some(l) => {
            set_language(l);
            true
        }
        None => false,
    }
}

/// The active UI language.
pub fn current() -> Lang {
    CURRENT.read().map(|g| *g).unwrap_or(Lang::En)
}

/// Translate an English UI string into the active language. Unknown strings
/// and English itself pass through unchanged.
pub fn t(s: &'static str) -> &'static str {
    let lang = current();
    let Some(row) = lang.locale_row() else {
        return s; // English (or an unmapped variant) → source string.
    };
    match SOURCE_STRINGS.iter().position(|src| *src == s) {
        Some(col) => LOCALES[row][col],
        None => s, // Not a translated string → leave as-is.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn codes_round_trip() {
        for l in Lang::ALL {
            assert_eq!(Lang::from_code(l.code()), Some(l));
        }
        assert_eq!(Lang::from_code("zh_TW"), Some(Lang::ZhTw));
        assert_eq!(Lang::from_code("klingon"), None);
    }

    #[test]
    fn locale_table_dimensions_match() {
        // 14 non-English rows, each aligned to SOURCE_STRINGS.
        assert_eq!(LOCALES.len(), 14);
        for row in LOCALES.iter() {
            assert_eq!(row.len(), SOURCE_STRINGS.len());
        }
    }

    #[test]
    #[serial]
    fn english_passes_through_and_unknown_is_identity() {
        set_language(Lang::En);
        assert_eq!(t("Settings"), "Settings");
        set_language(Lang::Zh);
        assert_eq!(
            t("a string that is not in the table"),
            "a string that is not in the table"
        );
        set_language(Lang::En);
    }

    #[test]
    #[serial]
    fn known_string_translates_when_language_set() {
        // "Settings" is in SOURCE_STRINGS; in zh it must differ from English.
        set_language(Lang::Zh);
        assert_ne!(t("Settings"), "Settings");
        set_language(Lang::En);
    }
}
