//! Central keybinding table. Used by the help overlay to list bindings;
//! the app dispatches keys directly (the table is the documented source of
//! truth). Phase 07 binds Ctrl+T / Ctrl+W (tabs) — listed here now so help
//! shows them.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionId {
    Submit,
    Newline,
    Interrupt,
    Quit,
    ClearScreen,
    ScrollUp,
    ScrollDown,
    OpenSettings,
    OpenHelp,
    OpenTasks,
    OpenSubAgents,
    ToggleSidebar,
    ToggleFold,
    ToggleYolo,
    NewTab,
    CloseTab,
    SwitchTab,
    CycleTab,
    Dismiss,
}

#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub keys: &'static str,
    pub action: ActionId,
    pub help: &'static str,
}

pub static KEYMAP: &[Binding] = &[
    Binding {
        keys: "Enter",
        action: ActionId::Submit,
        help: "Send message",
    },
    Binding {
        keys: "Shift/Alt+Enter or \\+Enter",
        action: ActionId::Newline,
        help: "Newline (Shift needs kitty-protocol terminals; \\+Enter works everywhere)",
    },
    Binding {
        keys: "Ctrl+C",
        action: ActionId::Interrupt,
        help: "Interrupt turn / quit when idle",
    },
    Binding {
        keys: "Ctrl+D",
        action: ActionId::Quit,
        help: "Quit",
    },
    Binding {
        keys: "Ctrl+L",
        action: ActionId::ClearScreen,
        help: "Clear screen",
    },
    Binding {
        keys: "PgUp",
        action: ActionId::ScrollUp,
        help: "Scroll up",
    },
    Binding {
        keys: "PgDn",
        action: ActionId::ScrollDown,
        help: "Scroll down",
    },
    Binding {
        keys: "Ctrl+O",
        action: ActionId::OpenSettings,
        help: "Settings (options)",
    },
    Binding {
        keys: "F1 / /help",
        action: ActionId::OpenHelp,
        help: "Help",
    },
    Binding {
        keys: "Ctrl+B",
        action: ActionId::OpenTasks,
        help: "Background tasks panel",
    },
    Binding {
        keys: "F2 / /subagents",
        action: ActionId::OpenSubAgents,
        help: "Sub-agent activity panel",
    },
    Binding {
        keys: "Ctrl+G",
        action: ActionId::ToggleSidebar,
        help: "Toggle the sidebar",
    },
    Binding {
        keys: "Ctrl+E",
        action: ActionId::ToggleFold,
        help: "Expand/collapse tool & thinking blocks",
    },
    Binding {
        keys: "Shift+Tab",
        action: ActionId::ToggleYolo,
        help: "Toggle bypass-approval mode",
    },
    Binding {
        keys: "Ctrl+T",
        action: ActionId::NewTab,
        help: "New session tab",
    },
    Binding {
        keys: "Ctrl+W",
        action: ActionId::CloseTab,
        help: "Close tab (quits if last)",
    },
    Binding {
        keys: "Ctrl+1..9",
        action: ActionId::SwitchTab,
        help: "Jump to tab N",
    },
    Binding {
        keys: "Ctrl+Tab",
        action: ActionId::CycleTab,
        help: "Cycle to next tab",
    },
    Binding {
        keys: "Esc",
        action: ActionId::Dismiss,
        help: "Close overlay",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keymap_has_core_bindings() {
        assert!(KEYMAP.iter().any(|b| b.action == ActionId::Submit));
        assert!(KEYMAP.iter().any(|b| b.action == ActionId::OpenSettings));
        assert!(KEYMAP.iter().any(|b| b.action == ActionId::OpenHelp));
        assert!(KEYMAP.iter().any(|b| b.action == ActionId::ToggleYolo));
    }
}
