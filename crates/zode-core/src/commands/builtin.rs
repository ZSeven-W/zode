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
        name: "image",
        description: "Attach or manage local images",
        usage: "/image <path>|list|remove <n>|clear",
        action: CommandAction::Ui,
    },
    SlashCommand {
        name: "vision",
        description: "Configure image understanding",
        usage: "/vision [mode|provider|model|prompt]",
        action: CommandAction::Ui,
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
        name: "plan",
        description: "Toggle plan mode (read-only; research then present a plan)",
        usage: "/plan",
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
    SlashCommand {
        name: "goal",
        description: "Set/clear a persistent objective in the system prompt",
        usage: "/goal [text]",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "effort",
        description: "Tune thoroughness: low | medium | high",
        usage: "/effort [low|medium|high]",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "copy",
        description: "Copy the last response to the clipboard",
        usage: "/copy",
        action: CommandAction::Ui,
    },
    SlashCommand {
        name: "export",
        description: "Export the conversation to a Markdown file",
        usage: "/export [path]",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "diff",
        description: "Show the working-tree git diff",
        usage: "/diff",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "agents",
        description: "List sub-agent types the Task tool can spawn",
        usage: "/agents",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "permissions",
        description: "Show tool permission rules (allow / ask / deny)",
        usage: "/permissions",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "hooks",
        description: "List configured hooks",
        usage: "/hooks",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "reload-plugins",
        description: "Re-read plugin state and rebuild the active session",
        usage: "/reload-plugins",
        action: CommandAction::Engine,
    },
    SlashCommand {
        name: "reload-skills",
        description: "Re-scan skills from disk and rebuild the active session",
        usage: "/reload-skills",
        action: CommandAction::Engine,
    },
];
