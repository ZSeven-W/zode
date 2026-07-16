//! /desktop slash-command parser. M1 exposes only `status` (bare `/desktop`
//! also maps to status; the TUI panel and consent modals are deferred to M4).

#[derive(Debug, PartialEq, Eq)]
pub enum DesktopCommand {
    Status,
}

pub fn map_subcommand(args: &str) -> Result<DesktopCommand, String> {
    match args.trim() {
        "" | "status" => Ok(DesktopCommand::Status),
        other => Err(format!("unknown subcommand {other:?}; usage: /desktop [status]")),
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
}
