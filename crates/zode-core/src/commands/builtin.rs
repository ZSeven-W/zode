//! Built-in command table. Behaviour lives in the front-ends; this is
//! just the descriptor list. New commands across phases append here.

use super::registry::{CommandAction, SlashCommand};

pub static BUILTINS: &[SlashCommand] = &[
    SlashCommand {
        name: "help",
        description: "Show commands and keybindings",
        usage: "/help",
        action: CommandAction::Local,
    },
    SlashCommand {
        name: "clear",
        description: "Clear the conversation context",
        usage: "/clear",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "exit",
        description: "Quit zode",
        usage: "/exit",
        action: CommandAction::Local,
    },
    SlashCommand {
        name: "model",
        description: "Show or switch the active model",
        usage: "/model [id]",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "config",
        description: "Print the effective config",
        usage: "/config",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "compact",
        description: "Summarize and compact the conversation",
        usage: "/compact",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "cost",
        description: "Show token usage and cost so far",
        usage: "/cost",
        action: CommandAction::Engine,
    },
    // Registered now, fully wired in later phases.
    SlashCommand {
        name: "theme",
        description: "Switch the TUI theme",
        usage: "/theme [id]",
        action: CommandAction::Ui,
    },
    SlashCommand {
        name: "sessions",
        description: "List and resume sessions",
        usage: "/sessions",
        action: CommandAction::Ui,
    },
    SlashCommand {
        name: "tab",
        description: "Switch session tab",
        usage: "/tab [n|next|prev]",
        action: CommandAction::Ui,
    },
    SlashCommand {
        name: "connect",
        description: "Connect a provider",
        usage: "/connect",
        action: CommandAction::Ui,
    },
    SlashCommand {
        name: "sidebar",
        description: "Show or hide the sidebar",
        usage: "/sidebar [on|off|toggle|auto]",
        action: CommandAction::Ui,
    },
    SlashCommand {
        name: "resume",
        description: "Resume a session by id",
        usage: "/resume <id>",
        action: CommandAction::Ui,
    },
    SlashCommand {
        name: "tasks",
        description: "Background shells + running turns",
        usage: "/tasks",
        action: CommandAction::Ui,
    },
    SlashCommand {
        name: "undo",
        description: "Undo the last file edit",
        usage: "/undo",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "redo",
        description: "Redo the last undone edit",
        usage: "/redo",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "yolo",
        description: "Toggle bypass-approval mode",
        usage: "/yolo",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "mcp",
        description: "List MCP servers",
        usage: "/mcp",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "skills",
        description: "List available skills",
        usage: "/skills",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "plugin",
        description: "Manage plugins (tools, MCP, skills, LSP): list / toggle",
        usage: "/plugin [id]",
        action: CommandAction::Ui,
    },
];
