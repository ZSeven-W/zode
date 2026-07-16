//! CLI argument definitions. Mirrors master plan §4.4.

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "zode", version, about = "AI-native coding CLI")]
pub struct Args {
    /// Optional subcommand (e.g. `zode doctor`). Absent = launch normally.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Headless single-turn mode: run this prompt, stream to stdout, exit.
    #[arg(short = 'p', long = "print")]
    pub print: Option<String>,

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
    /// Manage persisted sandbox state.
    Sandbox {
        #[command(subcommand)]
        action: SandboxCommand,
    },
}

#[derive(Debug, clap::Subcommand)]
pub enum SandboxCommand {
    /// Remove stale Windows sandbox capability ACEs and AppContainer profile.
    Cleanup,
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
    fn parses_yolo_and_sandbox() {
        let a = Args::parse_from(["zode", "--yolo", "--sandbox"]);
        assert!(a.yolo);
        assert!(a.sandbox);
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
}
