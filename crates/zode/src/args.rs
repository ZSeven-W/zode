//! CLI argument definitions. Mirrors master plan §4.4.

use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Plain,
    Json,
    StreamJson,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum PermissionModeArg {
    #[default]
    Default,
    DontAsk,
    AcceptEdits,
    Bypass,
}

#[derive(Debug, Parser)]
#[command(name = "zode", version, about = "AI-native coding CLI")]
pub struct Args {
    /// Optional subcommand (e.g. `zode doctor`). Absent = launch normally.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Headless single-turn mode: run this prompt, stream to stdout, exit.
    #[arg(short = 'p', long = "print")]
    pub print: Option<String>,

    /// Read the headless prompt from a UTF-8 file, or `-` for stdin.
    #[arg(long, conflicts_with_all = ["print", "prompt_json"])]
    pub prompt_file: Option<String>,

    /// Read a headless prompt from JSON (`{"prompt":"..."}` or a string).
    #[arg(long, conflicts_with_all = ["print", "prompt_file"])]
    pub prompt_json: Option<String>,

    /// Headless output contract. Structured formats reserve stdout for JSON.
    #[arg(long, value_enum, default_value_t)]
    pub output_format: OutputFormat,

    /// Maximum agentic model/tool turns for a headless run.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub max_turns: Option<u32>,

    /// Comma-separated tool names/globs to expose (empty means all).
    #[arg(long, value_delimiter = ',')]
    pub tools: Vec<String>,

    /// Comma-separated tool names/globs to remove after the allowlist.
    #[arg(long, value_delimiter = ',')]
    pub disallowed_tools: Vec<String>,

    /// Strict session id to create or resume (no prefix matching).
    #[arg(long, conflicts_with_all = ["continue_", "resume", "fork_session"])]
    pub session_id: Option<String>,

    /// Fork an existing session by exact id before running.
    #[arg(long, conflicts_with_all = ["continue_", "resume", "session_id"])]
    pub fork_session: Option<String>,

    /// Create an isolated Git worktree for the forked session.
    #[arg(long, requires = "fork_session")]
    pub fork_worktree: bool,

    /// Headless permission policy. `--yolo` remains an alias for bypass.
    #[arg(long, value_enum, default_value_t)]
    pub permission_mode: PermissionModeArg,

    /// Additional JSON permission rules file for this invocation.
    #[arg(long)]
    pub rules: Option<String>,

    /// Plain readline REPL instead of the full TUI.
    #[arg(long = "no-tui")]
    pub no_tui: bool,

    /// Continue the most recent session.
    #[arg(short = 'c', long = "continue")]
    pub continue_: bool,

    /// Resume a session by id (prefix-matched).
    #[arg(short = 'r', long = "resume")]
    pub resume: Option<String>,

    /// Model id override.
    #[arg(long)]
    pub model: Option<String>,

    /// Named provider from config.providers.
    #[arg(long)]
    pub provider: Option<String>,

    /// Working directory (defaults to current dir).
    #[arg(long)]
    pub cwd: Option<String>,

    /// Bypass interactive approval (deny rules still apply).
    #[arg(long)]
    pub yolo: bool,

    /// Run shell commands inside an OS sandbox (on by default; this flag is a
    /// no-op kept for compatibility — use `--no-sandbox` to disable).
    #[arg(long)]
    pub sandbox: bool,

    /// Disable the OS sandbox (it is on by default).
    #[arg(long)]
    pub no_sandbox: bool,

    /// Force-enable the browser tool group for this session.
    #[arg(long)]
    pub browser: bool,

    /// Disable the browser tool group for this session.
    #[arg(long)]
    pub no_browser: bool,

    /// Sandbox in read-only mode (deny all filesystem writes).
    #[arg(long)]
    pub sandbox_read_only: bool,

    /// Allow outbound network inside the sandbox (denied by default).
    #[arg(long)]
    pub sandbox_allow_network: bool,

    /// Strict read: also hide credential dirs (`~/.ssh`, `~/.aws`, the zode
    /// config, …) from reads. Off by default — a coding agent reads the repo.
    #[arg(long)]
    pub sandbox_strict_read: bool,

    /// Named sandbox policy (`read-only`, `workspace`, `workspace-network`,
    /// `unconfined`, or a config-defined sandbox.profiles entry).
    #[arg(long, conflicts_with_all = ["no_sandbox", "sandbox_read_only"])]
    pub sandbox_profile: Option<String>,

    /// Internal: run the extension-only event pump for Chrome Native Messaging.
    #[arg(long, hide = true)]
    pub browser_native_host: bool,
}

#[derive(Debug, clap::Args)]
pub struct ServerArgs {
    /// Transport endpoint URL: stdio://, ws://IP:PORT, or off.
    #[arg(long = "listen", default_value = "stdio://")]
    pub listen: String,
}

/// Subcommands. Kept minimal — the default (no subcommand) launches the CLI.
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Diagnose environment / config problems and check for a newer release.
    Doctor,
    /// Run zode as a JSON-RPC app server.
    Server(ServerArgs),
    /// Run Zode as an Agent Client Protocol (ACP) agent over stdio.
    Acp,
    /// Show local sessions, checkpoints, worktrees, and last run state.
    Dashboard {
        /// Emit a stable JSON snapshot instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Install, inspect, update, and remove plugin packages.
    Plugin {
        #[command(subcommand)]
        action: PluginCommand,
    },
    /// Manage persisted sandbox state.
    Sandbox {
        #[command(subcommand)]
        action: SandboxCommand,
    },
    /// Inspect, fork, rewind, and apply durable sessions.
    Session {
        #[command(subcommand)]
        action: SessionCommand,
    },
}

#[derive(Debug, clap::Subcommand)]
pub enum SandboxCommand {
    /// Remove stale Windows sandbox capability ACEs and AppContainer profile.
    Cleanup,
}

#[derive(Debug, clap::Subcommand)]
pub enum PluginCommand {
    /// List managed installed plugins, optionally including marketplace entries.
    List {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        available: bool,
    },
    /// Validate a plugin manifest and component paths.
    Validate {
        #[arg(default_value = ".")]
        path: String,
    },
    /// Install a local/Git plugin source or a configured marketplace entry.
    Install {
        source: String,
        /// Explicitly trust executable hooks, MCP servers, commands, and skills.
        #[arg(long)]
        trust: bool,
    },
    /// Update one plugin, or every managed plugin when name is omitted.
    Update {
        name: Option<String>,
        /// Accept manifest permissions broader than the installed snapshot
        /// (new network hosts, env vars, or context scopes).
        #[arg(long)]
        trust: bool,
    },
    /// Uninstall a managed plugin package.
    #[command(alias = "remove", alias = "rm")]
    Uninstall {
        name: String,
        #[arg(long)]
        keep_data: bool,
    },
    /// Enable an installed plugin package.
    Enable { name: String },
    /// Disable an installed plugin package without deleting it.
    Disable { name: String },
    /// Show provenance, hash, components, and install state.
    Details { name: String },
    /// Manage local/Git-backed static marketplace sources.
    Marketplace {
        #[command(subcommand)]
        action: MarketplaceCommand,
    },
}

#[derive(Debug, clap::Subcommand)]
pub enum MarketplaceCommand {
    /// List configured sources and their available plugins.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Add and cache a local/Git marketplace source.
    Add {
        source: String,
        #[arg(long)]
        trust: bool,
    },
    /// Refresh one source or all sources.
    Update { name: Option<String> },
    /// Remove a configured source cache (installed plugins remain installed).
    Remove { name: String },
}

#[derive(Debug, clap::Subcommand)]
pub enum SessionCommand {
    /// List sessions newest first.
    List {
        /// Emit a JSON array.
        #[arg(long)]
        json: bool,
    },
    /// Show durable V1 session metadata, journal head, and checkpoints.
    Show { id: String },
    /// Fork a session by exact id.
    Fork {
        id: String,
        /// Explicit target id (generated when omitted).
        #[arg(long)]
        target_id: Option<String>,
        /// Fork from the transcript state before this checkpoint's turn.
        #[arg(long)]
        checkpoint: Option<String>,
        /// Create a dedicated Git worktree for the fork.
        #[arg(long)]
        worktree: bool,
    },
    /// Preview or apply a checkpoint rewind.
    Rewind {
        id: String,
        checkpoint: String,
        /// Apply after conflict detection; omitted means preview only.
        #[arg(long)]
        apply: bool,
    },
    /// Apply an isolated session worktree's changes to another checkout.
    ApplyBack {
        id: String,
        /// Target checkout (defaults to the current directory).
        #[arg(long)]
        target: Option<String>,
    },
    /// Delete a session. Worktree removal requires an explicit flag.
    Delete {
        id: String,
        #[arg(long)]
        remove_worktree: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_print_flag() {
        let a = Args::parse_from(["zode", "-p", "hello"]);
        assert_eq!(a.print.as_deref(), Some("hello"));
    }

    #[test]
    fn parses_structured_headless_contract() {
        let args = Args::parse_from([
            "zode",
            "-p",
            "ship it",
            "--output-format",
            "stream-json",
            "--max-turns",
            "12",
            "--tools",
            "File*,Bash",
            "--disallowed-tools",
            "FileEdit",
            "--permission-mode",
            "dont-ask",
        ]);
        assert_eq!(args.output_format, OutputFormat::StreamJson);
        assert_eq!(args.max_turns, Some(12));
        assert_eq!(args.tools, ["File*", "Bash"]);
        assert_eq!(args.disallowed_tools, ["FileEdit"]);
        assert_eq!(args.permission_mode, PermissionModeArg::DontAsk);
    }

    #[test]
    fn parses_strict_session_and_fork_modes() {
        let strict = Args::parse_from(["zode", "-p", "x", "--session-id", "session-1"]);
        assert_eq!(strict.session_id.as_deref(), Some("session-1"));

        let fork = Args::parse_from([
            "zode",
            "-p",
            "x",
            "--fork-session",
            "source",
            "--fork-worktree",
        ]);
        assert_eq!(fork.fork_session.as_deref(), Some("source"));
        assert!(fork.fork_worktree);
    }

    #[test]
    fn prompt_sources_conflict() {
        assert!(Args::try_parse_from(["zode", "-p", "x", "--prompt-file", "prompt.txt"]).is_err());
    }

    #[test]
    fn parses_yolo_and_sandbox() {
        let a = Args::parse_from(["zode", "--yolo", "--sandbox"]);
        assert!(a.yolo);
        assert!(a.sandbox);
    }

    #[test]
    fn parses_named_sandbox_profile() {
        let args = Args::parse_from(["zode", "--sandbox-profile", "read-only"]);
        assert_eq!(args.sandbox_profile.as_deref(), Some("read-only"));
        assert!(
            Args::try_parse_from(["zode", "--sandbox-profile", "workspace", "--no-sandbox"])
                .is_err()
        );
    }

    #[test]
    fn parses_model_provider_resume() {
        let a = Args::parse_from([
            "zode",
            "--model",
            "MiniMax-M1",
            "--provider",
            "work",
            "-r",
            "abc123",
        ]);
        assert_eq!(a.model.as_deref(), Some("MiniMax-M1"));
        assert_eq!(a.provider.as_deref(), Some("work"));
        assert_eq!(a.resume.as_deref(), Some("abc123"));
    }

    #[test]
    fn continue_defaults_false() {
        let a = Args::parse_from(["zode"]);
        assert!(!a.continue_);
        assert!(!a.no_tui);
    }

    #[test]
    fn browser_flags_parse() {
        let a = Args::parse_from(["zode", "--browser"]);
        assert!(a.browser && !a.no_browser);
        let a = Args::parse_from(["zode", "--no-browser"]);
        assert!(!a.browser && a.no_browser);
    }

    #[test]
    fn native_host_flag_is_hidden_but_parseable() {
        let a = Args::parse_from(["zode", "--browser-native-host"]);
        assert!(a.browser_native_host);
    }

    #[test]
    fn parses_server_subcommand_default_stdio() {
        let a = Args::parse_from(["zode", "server"]);
        match a.command {
            Some(Command::Server(s)) => assert_eq!(s.listen, "stdio://"),
            other => panic!("expected server command, got {other:?}"),
        }
    }

    #[test]
    fn parses_server_listen_override() {
        let a = Args::parse_from(["zode", "server", "--listen", "ws://127.0.0.1:0"]);
        match a.command {
            Some(Command::Server(s)) => assert_eq!(s.listen, "ws://127.0.0.1:0"),
            other => panic!("expected server command, got {other:?}"),
        }
    }

    #[test]
    fn parses_sandbox_cleanup() {
        let args = Args::parse_from(["zode", "sandbox", "cleanup"]);
        assert!(matches!(
            args.command,
            Some(Command::Sandbox {
                action: SandboxCommand::Cleanup
            })
        ));
    }

    #[test]
    fn parses_session_rewind_and_fork() {
        let rewind = Args::parse_from(["zode", "session", "rewind", "s1", "cp1", "--apply"]);
        assert!(matches!(
            rewind.command,
            Some(Command::Session {
                action: SessionCommand::Rewind { apply: true, .. }
            })
        ));
        let fork = Args::parse_from([
            "zode",
            "session",
            "fork",
            "s1",
            "--checkpoint",
            "cp1",
            "--worktree",
        ]);
        assert!(matches!(
            fork.command,
            Some(Command::Session {
                action: SessionCommand::Fork { worktree: true, .. }
            })
        ));
    }

    #[test]
    fn readme_operational_commands_parse() {
        for argv in [
            vec!["zode", "acp"],
            vec!["zode", "dashboard", "--json"],
            vec!["zode", "session", "list", "--json"],
            vec!["zode", "session", "show", "session-1"],
            vec![
                "zode",
                "plugin",
                "install",
                "plugin@MARKETPLACE_NAME",
                "--trust",
            ],
            vec!["zode", "plugin", "marketplace", "list", "--json"],
        ] {
            Args::try_parse_from(argv).expect("README command should parse");
        }
    }
}
