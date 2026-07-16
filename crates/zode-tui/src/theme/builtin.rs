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
    vec![
        catppuccin_mocha(),
        aurora_forge(),
        ember_atelier(),
        sakura_paper(),
        arctic_day(),
        lavender_mist(),
        citrus_grove(),
        verdant_signal(),
        cyberpunk(),
        minimal(),
        hacker(),
    ]
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

pub fn aurora_forge() -> Theme {
    Theme {
        id: "aurora-forge".into(),
        name: "Aurora Forge".into(),
        description: "Deep navy with mint and violet accents".into(),
        bg_primary: Color::Indexed(233),
        bg_secondary: Color::Indexed(234),
        bg_input: Color::Indexed(235),
        fg_text: Color::Indexed(254),
        fg_subtle: Color::Indexed(66),
        fg_white: Color::Indexed(255),
        accent: Color::Indexed(80),
        accent_secondary: Color::Indexed(141),
        user: Color::Indexed(121),
        assistant: Color::Indexed(80),
        system: Color::Indexed(221),
        separator: Color::Indexed(80),
        icon_logo: "⟢".into(),
        icon_user: "❯".into(),
        icon_assistant: "◈".into(),
        icon_system: "⚡".into(),
        spinner_thinking: braille(),
        spinner_streaming: streaming(),
    }
}

pub fn ember_atelier() -> Theme {
    Theme {
        id: "ember-atelier".into(),
        name: "Ember Atelier".into(),
        description: "Warm charcoal with amber and coral accents".into(),
        bg_primary: Color::Indexed(233),
        bg_secondary: Color::Indexed(234),
        bg_input: Color::Indexed(235),
        fg_text: Color::Indexed(254),
        fg_subtle: Color::Indexed(138),
        fg_white: Color::Indexed(255),
        accent: Color::Indexed(215),
        accent_secondary: Color::Indexed(204),
        user: Color::Indexed(222),
        assistant: Color::Indexed(209),
        system: Color::Indexed(221),
        separator: Color::Indexed(215),
        icon_logo: "⟢".into(),
        icon_user: "❯".into(),
        icon_assistant: "◆".into(),
        icon_system: "⚡".into(),
        spinner_thinking: braille(),
        spinner_streaming: streaming(),
    }
}

pub fn sakura_paper() -> Theme {
    Theme {
        id: "sakura-paper".into(),
        name: "Sakura Paper".into(),
        description: "Warm paper with ink, sakura, and indigo accents".into(),
        bg_primary: Color::Indexed(255),
        bg_secondary: Color::Indexed(231),
        bg_input: Color::Indexed(254),
        fg_text: Color::Indexed(236),
        fg_subtle: Color::Indexed(243),
        fg_white: Color::Indexed(236),
        accent: Color::Indexed(125),
        accent_secondary: Color::Indexed(61),
        user: Color::Indexed(23),
        assistant: Color::Indexed(61),
        system: Color::Indexed(94),
        separator: Color::Indexed(125),
        icon_logo: "⟢".into(),
        icon_user: "❯".into(),
        icon_assistant: "●".into(),
        icon_system: "⚡".into(),
        spinner_thinking: braille(),
        spinner_streaming: streaming(),
    }
}

pub fn arctic_day() -> Theme {
    Theme {
        id: "arctic-day".into(),
        name: "Arctic Day".into(),
        description: "Cool white with ocean blue and teal accents".into(),
        bg_primary: Color::Indexed(255),
        bg_secondary: Color::Indexed(231),
        bg_input: Color::Indexed(254),
        fg_text: Color::Indexed(236),
        fg_subtle: Color::Indexed(242),
        fg_white: Color::Indexed(236),
        accent: Color::Indexed(25),
        accent_secondary: Color::Indexed(30),
        user: Color::Indexed(23),
        assistant: Color::Indexed(25),
        system: Color::Indexed(94),
        separator: Color::Indexed(25),
        icon_logo: "⟢".into(),
        icon_user: "❯".into(),
        icon_assistant: "◈".into(),
        icon_system: "⚡".into(),
        spinner_thinking: braille(),
        spinner_streaming: streaming(),
    }
}

pub fn lavender_mist() -> Theme {
    Theme {
        id: "lavender-mist".into(),
        name: "Lavender Mist".into(),
        description: "Soft lavender with plum and indigo accents".into(),
        bg_primary: Color::Indexed(189),
        bg_secondary: Color::Indexed(231),
        bg_input: Color::Indexed(225),
        fg_text: Color::Indexed(236),
        fg_subtle: Color::Indexed(60),
        fg_white: Color::Indexed(236),
        accent: Color::Indexed(61),
        accent_secondary: Color::Indexed(25),
        user: Color::Indexed(23),
        assistant: Color::Indexed(61),
        system: Color::Indexed(94),
        separator: Color::Indexed(61),
        icon_logo: "⟢".into(),
        icon_user: "❯".into(),
        icon_assistant: "●".into(),
        icon_system: "⚡".into(),
        spinner_thinking: braille(),
        spinner_streaming: streaming(),
    }
}

pub fn citrus_grove() -> Theme {
    Theme {
        id: "citrus-grove".into(),
        name: "Citrus Grove".into(),
        description: "Sunny cream with orange, olive, and teal accents".into(),
        bg_primary: Color::Indexed(230),
        bg_secondary: Color::Indexed(231),
        bg_input: Color::Indexed(229),
        fg_text: Color::Indexed(236),
        fg_subtle: Color::Indexed(240),
        fg_white: Color::Indexed(236),
        accent: Color::Indexed(130),
        accent_secondary: Color::Indexed(58),
        user: Color::Indexed(23),
        assistant: Color::Indexed(130),
        system: Color::Indexed(94),
        separator: Color::Indexed(130),
        icon_logo: "⟢".into(),
        icon_user: "❯".into(),
        icon_assistant: "◆".into(),
        icon_system: "⚡".into(),
        spinner_thinking: braille(),
        spinner_streaming: streaming(),
    }
}

pub fn verdant_signal() -> Theme {
    Theme {
        id: "verdant-signal".into(),
        name: "Verdant Signal".into(),
        description: "Forest black with leaf, mint, and gold accents".into(),
        bg_primary: Color::Indexed(232),
        bg_secondary: Color::Indexed(233),
        bg_input: Color::Indexed(234),
        fg_text: Color::Indexed(254),
        fg_subtle: Color::Indexed(66),
        fg_white: Color::Indexed(255),
        accent: Color::Indexed(114),
        accent_secondary: Color::Indexed(80),
        user: Color::Indexed(158),
        assistant: Color::Indexed(79),
        system: Color::Indexed(179),
        separator: Color::Indexed(114),
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
        assistant: Color::Indexed(147),
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
