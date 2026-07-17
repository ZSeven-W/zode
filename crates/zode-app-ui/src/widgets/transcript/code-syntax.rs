//! Minimal per-line tokenizer for fenced code block syntax highlighting.
//!
//! This is deliberately not a full language grammar: no external tokenizer
//! or grammar dependency, and no cross-line state. Each source line is
//! classified independently, so multi-line constructs - a Rust/JS block
//! comment (`/* ... */`) or a Python triple-quoted string - are not
//! recognized as one token; a per-line scan sees their delimiters as plain
//! punctuation instead. That tradeoff keeps the implementation small and
//! dependency-free while still covering the common case (short pasted
//! snippets, single-line comments and strings).

/// One highlighted span's semantic class. `Plain` covers identifiers and
/// whitespace - callers paint it with the code block's ordinary foreground
/// color, same as before highlighting existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TokenClass {
    Keyword,
    String,
    Comment,
    Number,
    Punctuation,
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    Rust,
    JavaScript,
    Python,
    Json,
    Bash,
    Plain,
}

const RUST_KEYWORDS: &[&str] = &[
    "fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "use", "mod", "return", "if",
    "else", "match", "for", "while", "loop", "break", "continue", "self", "Self", "true", "false",
    "const", "static", "async", "await", "move", "ref", "dyn", "where", "as", "in", "unsafe",
    "super", "crate", "type",
];

const JAVASCRIPT_KEYWORDS: &[&str] = &[
    "function",
    "const",
    "let",
    "var",
    "return",
    "if",
    "else",
    "for",
    "while",
    "do",
    "break",
    "continue",
    "class",
    "extends",
    "new",
    "this",
    "super",
    "import",
    "export",
    "from",
    "default",
    "async",
    "await",
    "try",
    "catch",
    "finally",
    "throw",
    "typeof",
    "instanceof",
    "in",
    "of",
    "true",
    "false",
    "null",
    "undefined",
    "interface",
    "type",
    "enum",
    "implements",
    "public",
    "private",
    "protected",
    "static",
    "readonly",
];

const PYTHON_KEYWORDS: &[&str] = &[
    "def", "return", "if", "elif", "else", "for", "while", "break", "continue", "class", "import",
    "from", "as", "with", "try", "except", "finally", "raise", "pass", "lambda", "None", "True",
    "False", "and", "or", "not", "in", "is", "yield", "async", "await", "global", "nonlocal",
    "del", "assert",
];

const JSON_KEYWORDS: &[&str] = &["true", "false", "null"];

const BASH_KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "function",
    "return", "local", "export", "echo", "in", "break", "continue",
];

fn classify_language(info: &str) -> Language {
    match info.trim().to_ascii_lowercase().as_str() {
        "rust" | "rs" => Language::Rust,
        "javascript" | "js" | "jsx" | "mjs" | "cjs" | "typescript" | "ts" | "tsx" => {
            Language::JavaScript
        }
        "python" | "py" | "py3" => Language::Python,
        "json" | "jsonc" => Language::Json,
        "bash" | "sh" | "shell" | "zsh" | "console" => Language::Bash,
        _ => Language::Plain,
    }
}

fn keywords(language: Language) -> &'static [&'static str] {
    match language {
        Language::Rust => RUST_KEYWORDS,
        Language::JavaScript => JAVASCRIPT_KEYWORDS,
        Language::Python => PYTHON_KEYWORDS,
        Language::Json => JSON_KEYWORDS,
        Language::Bash => BASH_KEYWORDS,
        Language::Plain => &[],
    }
}

/// Line-comment prefixes recognized for the language. Block comments are
/// out of scope (see module docs); JSON has none. The `Plain` fallback
/// accepts both common styles since the source language is unknown.
fn comment_starts(language: Language) -> &'static [&'static str] {
    match language {
        Language::Rust | Language::JavaScript => &["//"],
        Language::Python | Language::Bash => &["#"],
        Language::Json => &[],
        Language::Plain => &["//", "#"],
    }
}

/// Tokenizes one source line (no trailing newline) into ordered
/// `(class, text)` spans whose texts concatenate back to `line` exactly -
/// callers rely on this to preserve glyph positions when painting each span
/// at an accumulated x-offset instead of drawing the line as one string.
pub(super) fn tokenize_line(language: &str, line: &str) -> Vec<(TokenClass, String)> {
    let language = classify_language(language);
    let keyword_set = keywords(language);
    let comment_prefixes = comment_starts(language);
    let chars: Vec<char> = line.chars().collect();
    let mut tokens: Vec<(TokenClass, String)> = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if starts_with_any(&chars[index..], comment_prefixes) {
            push(
                &mut tokens,
                TokenClass::Comment,
                chars[index..].iter().collect(),
            );
            break;
        }
        let ch = chars[index];
        if ch == '"' || ch == '\'' || ch == '`' {
            let start = index;
            index += 1;
            while index < chars.len() && chars[index] != ch {
                if chars[index] == '\\' && index + 1 < chars.len() {
                    index += 1;
                }
                index += 1;
            }
            if index < chars.len() {
                index += 1; // consume the closing quote
            }
            push(
                &mut tokens,
                TokenClass::String,
                chars[start..index].iter().collect(),
            );
            continue;
        }
        if ch.is_ascii_digit() {
            let start = index;
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric()
                    || chars[index] == '.'
                    || chars[index] == '_')
            {
                index += 1;
            }
            push(
                &mut tokens,
                TokenClass::Number,
                chars[start..index].iter().collect(),
            );
            continue;
        }
        if ch.is_alphabetic() || ch == '_' {
            let start = index;
            while index < chars.len() && (chars[index].is_alphanumeric() || chars[index] == '_') {
                index += 1;
            }
            let word: String = chars[start..index].iter().collect();
            let class = if keyword_set.contains(&word.as_str()) {
                TokenClass::Keyword
            } else {
                TokenClass::Plain
            };
            push(&mut tokens, class, word);
            continue;
        }
        if ch.is_whitespace() {
            let start = index;
            while index < chars.len() && chars[index].is_whitespace() {
                index += 1;
            }
            push(
                &mut tokens,
                TokenClass::Plain,
                chars[start..index].iter().collect(),
            );
            continue;
        }
        push(&mut tokens, TokenClass::Punctuation, ch.to_string());
        index += 1;
    }
    tokens
}

/// Appends `text` as a new span, or extends the previous span in place when
/// it shares the same class - keeps adjacent punctuation (`--`, `==`) and
/// adjacent plain runs (a word directly followed by a space) as one paint
/// call instead of one per character.
fn push(tokens: &mut Vec<(TokenClass, String)>, class: TokenClass, text: String) {
    if text.is_empty() {
        return;
    }
    if let Some((last_class, last_text)) = tokens.last_mut() {
        if *last_class == class {
            last_text.push_str(&text);
            return;
        }
    }
    tokens.push((class, text));
}

fn starts_with_any(chars: &[char], prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| {
        let mut prefix_chars = prefix.chars();
        chars
            .iter()
            .zip(prefix_chars.by_ref())
            .all(|(a, b)| *a == b)
            && prefix_chars.next().is_none()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(tokens: &[(TokenClass, String)]) -> String {
        tokens.iter().map(|(_, text)| text.as_str()).collect()
    }

    #[test]
    fn token_texts_concatenate_back_to_the_source_line_exactly() {
        for (language, line) in [
            ("rust", "let x = \"hi\"; // note"),
            ("js", "const x = 1 + 2; // sum"),
            ("python", "def f(x): return x  # id"),
            ("json", "{\"a\": 1, \"b\": true}"),
            ("bash", "echo \"$HOME\" # print"),
            ("", "no known language here"),
        ] {
            let tokens = tokenize_line(language, line);
            assert_eq!(text_of(&tokens), line, "language={language}");
        }
    }

    #[test]
    fn rust_keywords_and_strings_are_classified() {
        let tokens = tokenize_line("rust", "let mut x = \"hi\";");
        assert!(tokens
            .iter()
            .any(|(class, text)| *class == TokenClass::Keyword && text == "let"));
        assert!(tokens
            .iter()
            .any(|(class, text)| *class == TokenClass::Keyword && text == "mut"));
        assert!(tokens
            .iter()
            .any(|(class, text)| *class == TokenClass::String && text == "\"hi\""));
        // "x" merges with its trailing space into one Plain span (adjacent
        // same-class runs coalesce - see `push`), so match on the trimmed
        // text rather than an exact "x".
        assert!(tokens
            .iter()
            .any(|(class, text)| *class == TokenClass::Plain && text.trim() == "x"));
    }

    #[test]
    fn comment_prefix_ends_the_line_as_one_comment_span() {
        let tokens = tokenize_line("rust", "let x = 1; // trailing note");
        let (class, text) = tokens.last().unwrap();
        assert_eq!(*class, TokenClass::Comment);
        assert_eq!(text, "// trailing note");
    }

    #[test]
    fn numbers_are_classified_distinctly_from_identifiers() {
        let tokens = tokenize_line("python", "value = 42");
        assert!(tokens
            .iter()
            .any(|(class, text)| *class == TokenClass::Number && text == "42"));
    }

    #[test]
    fn unknown_language_still_highlights_strings_and_comments() {
        let tokens = tokenize_line("toml", "name = \"zode\" # comment");
        assert!(tokens
            .iter()
            .any(|(class, text)| *class == TokenClass::String && text == "\"zode\""));
        assert!(tokens
            .iter()
            .any(|(class, text)| *class == TokenClass::Comment && text == "# comment"));
    }
}
