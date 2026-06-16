//! CLI argument definitions. Mirrors master plan §4.4.

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "zode", version, about = "AI-native coding CLI")]
pub struct Args {
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

    /// Sandbox in read-only mode (deny all filesystem writes).
    #[arg(long)]
    pub sandbox_read_only: bool,

    /// Allow outbound network inside the sandbox (denied by default).
    #[arg(long)]
    pub sandbox_allow_network: bool,
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
}
