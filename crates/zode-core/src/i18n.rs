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

/// Hand-maintained translations for UI strings added after the bulk locale
/// table was generated (so new strings don't require regenerating the big
/// index-aligned arrays). Each row is `[en, zh, zh-tw, ja, ko, es, fr, de, pt,
/// ru, hi, id, th, tr, vi]`, aligned to [`Lang::ALL`]. Checked before
/// `SOURCE_STRINGS`. Key names (enter/esc) are intentionally left out — they
/// pass through unchanged.
#[rustfmt::skip]
const EXTRA: &[[&str; 15]] = &[
    // /connect dialog.
    ["Configured", "已配置", "已設定", "設定済み", "구성됨", "Configurados", "Configurés", "Konfiguriert", "Configurados", "Настроенные", "कॉन्फ़िगर किए गए", "Terkonfigurasi", "ที่กำหนดค่าแล้ว", "Yapılandırılmış", "Đã cấu hình"],
    ["No providers", "无提供商", "無供應商", "プロバイダーなし", "공급자 없음", "Sin proveedores", "Aucun fournisseur", "Keine Anbieter", "Sem provedores", "Нет провайдеров", "कोई प्रदाता नहीं", "Tidak ada penyedia", "ไม่มีผู้ให้บริการ", "Sağlayıcı yok", "Không có nhà cung cấp"],
    // NB: "Provider" intentionally omitted — it already lives in SOURCE_STRINGS
    // (used by settings); duplicating it here would silently override that.
    ["API key", "API 密钥", "API 金鑰", "API キー", "API 키", "Clave API", "Clé API", "API-Schlüssel", "Chave API", "API-ключ", "API कुंजी", "Kunci API", "คีย์ API", "API anahtarı", "Khóa API"],
    ["type", "类型", "類型", "タイプ", "유형", "tipo", "type", "Typ", "tipo", "тип", "प्रकार", "tipe", "ประเภท", "tür", "loại"],
    ["model", "模型", "模型", "モデル", "모델", "modelo", "modèle", "Modell", "modelo", "модель", "मॉडल", "model", "โมเดล", "model", "mô hình"],
    ["base URL", "基础 URL", "基礎 URL", "ベース URL", "기본 URL", "URL base", "URL de base", "Basis-URL", "URL base", "Базовый URL", "बेस URL", "URL dasar", "URL พื้นฐาน", "Temel URL", "URL cơ sở"],
    ["context", "上下文", "上下文", "コンテキスト", "컨텍스트", "contexto", "contexte", "Kontext", "contexto", "контекст", "संदर्भ", "konteks", "บริบท", "bağlam", "ngữ cảnh"],
    ["max output", "最大输出", "最大輸出", "最大出力", "최대 출력", "salida máx.", "sortie max", "max. Ausgabe", "saída máx.", "макс. вывод", "अधिकतम आउटपुट", "keluaran maks", "เอาต์พุตสูงสุด", "maks çıktı", "đầu ra tối đa"],
    ["input $/M", "输入 $/M", "輸入 $/M", "入力 $/M", "입력 $/M", "entrada $/M", "entrée $/M", "Eingabe $/M", "entrada $/M", "ввод $/M", "इनपुट $/M", "input $/M", "อินพุต $/M", "girdi $/M", "đầu vào $/M"],
    ["output $/M", "输出 $/M", "輸出 $/M", "出力 $/M", "출력 $/M", "salida $/M", "sortie $/M", "Ausgabe $/M", "saída $/M", "вывод $/M", "आउटपुट $/M", "output $/M", "เอาต์พุต $/M", "çıktı $/M", "đầu ra $/M"],
    ["submit", "提交", "提交", "送信", "제출", "enviar", "envoyer", "senden", "enviar", "отправить", "सबमिट करें", "kirim", "ส่ง", "gönder", "gửi"],
];

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
    // Hand-maintained overlay first (new UI strings), then the generated table.
    if let Some(row) = EXTRA.iter().find(|r| r[0] == s) {
        let col = Lang::ALL.iter().position(|l| *l == lang).unwrap_or(0);
        return row[col];
    }
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
    fn overlay_translates_new_keys() {
        set_language(Lang::Zh);
        assert_eq!(t("model"), "模型");
        assert_eq!(t("Configured"), "已配置");
        assert_eq!(t("max output"), "最大输出");
        set_language(Lang::En);
        assert_eq!(t("model"), "model"); // English column = passthrough
        assert_eq!(t("Configured"), "Configured");
    }

    #[test]
    fn overlay_rows_have_no_duplicate_keys() {
        for (i, a) in EXTRA.iter().enumerate() {
            for b in EXTRA.iter().skip(i + 1) {
                assert_ne!(a[0], b[0], "duplicate overlay key {:?}", a[0]);
            }
        }
    }

    #[test]
    fn overlay_keys_do_not_shadow_generated_table() {
        // An EXTRA key that also exists in SOURCE_STRINGS would silently override
        // the generated translation everywhere it's used — forbid it.
        for row in EXTRA {
            assert!(
                !SOURCE_STRINGS.contains(&row[0]),
                "overlay key {:?} shadows the generated table; remove it from EXTRA",
                row[0]
            );
        }
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
