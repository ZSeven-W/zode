//! Team playbook — the L3 layer is pure prompt guidance (spec §6): three
//! collaboration plays the leader model chooses from autonomously. Injected
//! into the system prompt only when the team tool group is actually
//! registered (never in plan mode; never dependent on transient lock state).

/// `external_agents`: `(name, strength summary)` pairs, used so the leader
/// assigns roles by each CLI's strong suit.
pub fn render_playbook(external_agents: &[(String, String)]) -> String {
    let mut out = String::from(
        "\n## Team plays\n\
         You can hire persistent teammates (TeamHire) and coordinate them:\n\
         - 流水线 pipeline: split roles by strength (design → implement → review); \
         relay review findings back into the implementer's session (TeamSend) and \
         loop until the reviewer passes it.\n\
         - 辩论 debate: give the same problem to several teammates independently, \
         cross-relay their answers for critique (@ask relay), then synthesize a \
         ruling yourself. Prefer heterogeneous backends — external CLIs and \
         internal teammates on different providers catch each other's blind spots.\n\
         - 蜂群 swarm: partition non-overlapping file scopes via `TeamSend` \
         `claims` BEFORE parallel work; each teammate records conclusions to the \
         board when done (or reports them for you to record).\n\
         Board discipline: keep the goal and task assignments on the board \
         (TeamBoardUpdate, CAS revision); record durable conclusions with \
         TeamBoardAppend; relay `@ask` lines you receive in tool results.\n",
    );
    if !external_agents.is_empty() {
        out.push_str("External teammates available: ");
        out.push_str(
            &external_agents
                .iter()
                .map(|(n, d)| format!("{n} — {d}"))
                .collect::<Vec<_>>()
                .join("; "),
        );
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playbook_mentions_three_plays_and_claims() {
        let p = render_playbook(&[("codex".into(), "deep debugging".into())]);
        for kw in ["流水线", "辩论", "蜂群", "claims", "@ask", "codex"] {
            assert!(p.contains(kw), "{kw}");
        }
    }
}
