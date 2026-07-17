//! Teammate preambles: identity, roster, board protocol, @ask convention.
//! Internal teammates get this as a system-prompt suffix and use the
//! identity-bound team tools; external teammates get it as a prompt prefix
//! and receive board content inline from the leader (they have no zode
//! tools).

use super::TeammateSnapshot;

/// How this teammate reaches the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardAccess {
    /// Internal teammate: use team_board_read/update/append + team_claim.
    Tools,
    /// External teammate: the leader inlines board summaries into sends and
    /// writes conclusions back on the teammate's behalf.
    Inline,
}

pub fn render_preamble(
    name: &str,
    role: &str,
    goal: &str,
    roster: &[TeammateSnapshot],
    board_access: BoardAccess,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "You are teammate '{name}' (role: {role}) on a zode agent team.\n"
    ));
    if !goal.is_empty() {
        out.push_str(&format!("Team goal: {goal}\n"));
    }
    if !roster.is_empty() {
        out.push_str("Roster: ");
        out.push_str(
            &roster
                .iter()
                .map(|t| format!("{} ({})", t.name, t.role))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push('\n');
    }
    match board_access {
        BoardAccess::Tools => out.push_str(
            "Board protocol: before starting, read the shared board \
             (team_board_read); claim the files you will touch (team_claim) \
             and stay inside your claims; when done, record conclusions \
             (team_board_append) and release claims you no longer need.\n",
        ),
        BoardAccess::Inline => out.push_str(
            "Board protocol: the team lead includes the current board state \
             in each task message; report your conclusions clearly in your \
             reply so the lead can record them on the board.\n",
        ),
    }
    out.push_str(
        "To ask a teammate something, end your reply with a line exactly of \
         the form `@ask <name>: <question>` — the team lead relays it.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preamble_mentions_identity_board_and_ask() {
        let roster = vec![TeammateSnapshot {
            name: "bob".into(),
            role: "reviewer".into(),
            model_label: "internal".into(),
            status_line: "idle".into(),
            usage_in: 0,
            usage_out: 0,
        }];
        let p = render_preamble(
            "alice",
            "implementer",
            "修复登录",
            &roster,
            BoardAccess::Tools,
        );
        for needle in [
            "alice",
            "implementer",
            "修复登录",
            "bob",
            "team_claim",
            "@ask",
        ] {
            assert!(p.contains(needle), "missing {needle}");
        }
        let p2 = render_preamble("codex-1", "builder", "", &[], BoardAccess::Inline);
        assert!(p2.contains("team lead includes"));
        assert!(!p2.contains("team_board_read"));
    }
}
