//! `/team` slash-command parser. Bare `/team` and `/team status` report the
//! roster; `/team board` prints the board; `/team dismiss <name>` removes a
//! teammate. The TUI dispatches these against the engine's `TeamManager`.

#[derive(Debug, PartialEq, Eq)]
pub enum TeamCommand {
    /// Roster + status report (bare `/team` and `/team status`).
    Status,
    Board,
    Dismiss(String),
}

pub fn parse(input: &str) -> Option<TeamCommand> {
    let rest = input.strip_prefix("/team")?;
    // Must be exactly `/team` or `/team ` + args (not `/teamx`).
    let rest = match rest {
        "" => "",
        r if r.starts_with(char::is_whitespace) => r.trim(),
        _ => return None,
    };
    let (head, arg) = match rest.split_once(char::is_whitespace) {
        Some((h, a)) => (h, a.trim()),
        None => (rest, ""),
    };
    match head {
        "" | "status" => Some(TeamCommand::Status),
        "board" => Some(TeamCommand::Board),
        "dismiss" if !arg.is_empty() => Some(TeamCommand::Dismiss(arg.to_string())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_command_parsing() {
        assert_eq!(parse("/team"), Some(TeamCommand::Status));
        assert_eq!(parse("/team status"), Some(TeamCommand::Status));
        assert_eq!(parse("/team board"), Some(TeamCommand::Board));
        assert_eq!(
            parse("/team dismiss codex-impl"),
            Some(TeamCommand::Dismiss("codex-impl".to_string()))
        );
        assert_eq!(parse("/team dismiss"), None, "dismiss needs a name");
        assert_eq!(parse("/teamx"), None, "must be an exact command");
        assert_eq!(parse("/other"), None);
    }
}
