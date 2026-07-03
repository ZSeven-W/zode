//! /browser slash-command parser. Bare `/browser` opens the TUI panel;
//! subcommands are the scriptable fast path.

#[derive(Debug, PartialEq, Eq)]
pub enum BrowserCommand {
    Panel,
    Status,
    Launch,
    Close,
    Pair,
    Target { target: String },
    Screenshot { path: Option<String> },
}

pub fn map_subcommand(args: &str) -> Result<BrowserCommand, String> {
    let trimmed = args.trim();
    let (head, rest) = match trimmed.split_once(char::is_whitespace) {
        Some((h, r)) => (h, r.trim()),
        None => (trimmed, ""),
    };
    match head {
        "" => Ok(BrowserCommand::Panel),
        "status" => Ok(BrowserCommand::Status),
        "launch" => Ok(BrowserCommand::Launch),
        "close" => Ok(BrowserCommand::Close),
        "pair" => Ok(BrowserCommand::Pair),
        "target" => match rest {
            "managed" | "bridge" => Ok(BrowserCommand::Target {
                target: rest.to_string(),
            }),
            _ => Err("usage: /browser target <managed|bridge>".to_string()),
        },
        "screenshot" => Ok(BrowserCommand::Screenshot {
            path: (!rest.is_empty()).then(|| rest.to_string()),
        }),
        other => Err(format!(
            "unknown subcommand {other:?}; usage: /browser [status|launch|close|pair|target <managed|bridge>|screenshot [path]]"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_opens_panel() {
        assert_eq!(map_subcommand(""), Ok(BrowserCommand::Panel));
        assert_eq!(map_subcommand("   "), Ok(BrowserCommand::Panel));
    }

    #[test]
    fn subcommands_parse() {
        assert_eq!(map_subcommand("status"), Ok(BrowserCommand::Status));
        assert_eq!(map_subcommand("launch"), Ok(BrowserCommand::Launch));
        assert_eq!(map_subcommand("close"), Ok(BrowserCommand::Close));
        assert_eq!(map_subcommand("pair"), Ok(BrowserCommand::Pair));
        assert_eq!(
            map_subcommand("target bridge"),
            Ok(BrowserCommand::Target {
                target: "bridge".into()
            })
        );
        assert_eq!(
            map_subcommand("screenshot"),
            Ok(BrowserCommand::Screenshot { path: None })
        );
        assert_eq!(
            map_subcommand("screenshot /tmp/a.jpg"),
            Ok(BrowserCommand::Screenshot {
                path: Some("/tmp/a.jpg".into())
            })
        );
    }

    #[test]
    fn errors_are_actionable() {
        let e = map_subcommand("target").unwrap_err();
        assert!(e.contains("managed|bridge"));
        let e = map_subcommand("frobnicate").unwrap_err();
        assert!(e.contains("usage"), "{e}");
    }
}
