//! /desktop slash-command parser. `status` reports permission/health; `attach`
//! connects a CDP backend to a running Electron/Chromium debug port (enables
//! `DesktopEval`). The TUI panel and consent modals are deferred to M4.

#[derive(Debug, PartialEq, Eq)]
pub enum DesktopCommand {
    Status,
    /// Attach a CDP backend to `127.0.0.1:<port>`.
    Attach {
        port: u16,
    },
}

pub fn map_subcommand(args: &str) -> Result<DesktopCommand, String> {
    let trimmed = args.trim();
    let (head, rest) = match trimmed.split_once(char::is_whitespace) {
        Some((h, r)) => (h, r.trim()),
        None => (trimmed, ""),
    };
    match head {
        "" | "status" => Ok(DesktopCommand::Status),
        "attach" => {
            let port: u16 = rest
                .parse()
                .map_err(|_| "usage: /desktop attach <port>".to_string())?;
            Ok(DesktopCommand::Attach { port })
        }
        other => Err(format!(
            "unknown subcommand {other:?}; usage: /desktop [status|attach <port>]"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_parses_and_unknown_errors() {
        assert_eq!(map_subcommand("status"), Ok(DesktopCommand::Status));
        assert_eq!(map_subcommand(""), Ok(DesktopCommand::Status));
        assert_eq!(map_subcommand("  "), Ok(DesktopCommand::Status));
        assert!(map_subcommand("frobnicate").is_err());
    }

    #[test]
    fn attach_parses_port() {
        assert_eq!(
            map_subcommand("attach 9222"),
            Ok(DesktopCommand::Attach { port: 9222 })
        );
        assert!(map_subcommand("attach notaport").is_err());
        assert!(map_subcommand("attach").is_err());
    }
}
