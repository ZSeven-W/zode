//! Built-in themes, translated from the Zig theme design doc
//! (docs/superpowers/specs/2026-04-02-tui-theme-system-design.md).

use ratatui::style::Color;

use super::Theme;

fn braille() -> Vec<String> {
    ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn streaming() -> Vec<String> {
    ["◐", "◓", "◑", "◒"].iter().map(|s| s.to_string()).collect()
}

pub fn all() -> Vec<Theme> {
    vec![catppuccin_mocha(), cyberpunk(), minimal(), hacker()]
}

pub fn catppuccin_mocha() -> Theme {
    Theme {
        id: "catppuccin-mocha".into(),
        name: "Catppuccin Mocha".into(),
        description: "Elegant mocha (default)".into(),
        bg_primary: Color::Indexed(235),
        bg_secondary: Color::Indexed(236),
        bg_input: Color::Indexed(237),
        fg_text: Color::Indexed(252),
        fg_subtle: Color::Indexed(245),
        fg_white: Color::Indexed(255),
        accent: Color::Indexed(141),
        accent_secondary: Color::Indexed(111),
        user: Color::Indexed(114),
        assistant: Color::Indexed(111),
        system: Color::Indexed(221),
        separator: Color::Indexed(141),
        icon_logo: "⟢".into(),
        icon_user: "❯".into(),
        icon_assistant: "◈".into(),
        icon_system: "⚡".into(),
        spinner_thinking: braille(),
        spinner_streaming: streaming(),
    }
}

pub fn cyberpunk() -> Theme {
    Theme {
        id: "cyberpunk".into(),
        name: "Cyberpunk".into(),
        description: "Neon gradients".into(),
        bg_primary: Color::Indexed(233),
        bg_secondary: Color::Indexed(234),
        bg_input: Color::Indexed(235),
        fg_text: Color::Indexed(252),
        fg_subtle: Color::Indexed(245),
        fg_white: Color::Indexed(255),
        accent: Color::Indexed(201),
        accent_secondary: Color::Indexed(51),
        user: Color::Indexed(114),
        assistant: Color::Indexed(111),
        system: Color::Indexed(221),
        separator: Color::Indexed(201),
        icon_logo: "⟢".into(),
        icon_user: "❯".into(),
        icon_assistant: "◆".into(),
        icon_system: "⚡".into(),
        spinner_thinking: braille(),
        spinner_streaming: streaming(),
    }
}

pub fn minimal() -> Theme {
    Theme {
        id: "minimal".into(),
        name: "Minimal".into(),
        description: "Tokyo Night inspired".into(),
        bg_primary: Color::Indexed(234),
        bg_secondary: Color::Indexed(235),
        bg_input: Color::Indexed(236),
        fg_text: Color::Indexed(252),
        fg_subtle: Color::Indexed(60),
        fg_white: Color::Indexed(255),
        accent: Color::Indexed(111),
        accent_secondary: Color::Indexed(60),
        user: Color::Indexed(111),
        assistant: Color::Indexed(111),
        system: Color::Indexed(221),
        separator: Color::Indexed(111),
        icon_logo: "⟢".into(),
        icon_user: "❯".into(),
        icon_assistant: "●".into(),
        icon_system: "⚡".into(),
        spinner_thinking: braille(),
        spinner_streaming: streaming(),
    }
}

pub fn hacker() -> Theme {
    Theme {
        id: "hacker".into(),
        name: "Hacker".into(),
        description: "Matrix green".into(),
        bg_primary: Color::Indexed(232),
        bg_secondary: Color::Indexed(233),
        bg_input: Color::Indexed(234),
        fg_text: Color::Indexed(252),
        fg_subtle: Color::Indexed(22),
        fg_white: Color::Indexed(255),
        accent: Color::Indexed(46),
        accent_secondary: Color::Indexed(22),
        user: Color::Indexed(46),
        assistant: Color::Indexed(46),
        system: Color::Indexed(46),
        separator: Color::Indexed(46),
        icon_logo: "⟢".into(),
        icon_user: "❯".into(),
        icon_assistant: "▶".into(),
        icon_system: "⚡".into(),
        spinner_thinking: braille(),
        spinner_streaming: streaming(),
    }
}
