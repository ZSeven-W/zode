//! Cross-agent portability checks.
//!
//! Zode merges skills and slash commands discovered from other coding agents
//! (Claude / codex / opencode / antigravity / …). Some of those items hard-code
//! environment variables that only the defining host ever sets — e.g. a Claude
//! plugin command that runs `${CLAUDE_PLUGIN_ROOT}/scripts/setup.sh`. Under
//! zode that variable is unset, so the item can only fail. Rather than surface
//! a command/skill that is broken on arrival, we drop it at load time. Zode's
//! own items never reference these foreign host variables, so the filter only
//! ever removes non-portable cross-agent content.

/// Environment variables tied to a specific foreign agent's plugin host. Zode
/// never sets these, so any skill/command body that interpolates one is
/// non-portable. This is an allowlist of *known-foreign* names — generic shell
/// variables (`$HOME`, `$PWD`, positional `$1`, …) are intentionally absent and
/// therefore pass through untouched.
const FOREIGN_HOST_VARS: &[&str] = &[
    "CLAUDE_PLUGIN_ROOT",
    "CLAUDE_PROJECT_DIR",
    "CLAUDE_CONFIG_DIR",
    "CODEX_PLUGIN_ROOT",
    "CODEX_HOME",
    "OPENCODE_PLUGIN_ROOT",
    "ANTIGRAVITY_PLUGIN_ROOT",
    "KILO_PLUGIN_ROOT",
    "CURSOR_PLUGIN_ROOT",
    "PI_PLUGIN_ROOT",
];

/// If `text` interpolates a foreign host variable as `${VAR}` or `$VAR`, return
/// that variable's name; otherwise `None`. Callers use the returned name only
/// for a debug log explaining why an item was dropped.
pub fn foreign_host_var(text: &str) -> Option<&'static str> {
    FOREIGN_HOST_VARS
        .iter()
        .copied()
        .find(|var| text.contains(&format!("${{{var}}}")) || mentions_bare(text, var))
}

/// True if `text` contains `$VAR` as a standalone shell reference, i.e. `$VAR`
/// is not immediately followed by another identifier character. This keeps
/// `$CODEX_HOME` from matching inside `$CODEX_HOMELAB`.
fn mentions_bare(text: &str, var: &str) -> bool {
    let needle = format!("${var}");
    let mut from = 0;
    while let Some(rel) = text[from..].find(&needle) {
        let end = from + rel + needle.len();
        let next_is_ident = text[end..]
            .chars()
            .next()
            .map(|c| c.is_ascii_alphanumeric() || c == '_')
            .unwrap_or(false);
        if !next_is_ident {
            return true;
        }
        from = end;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_braced_foreign_var() {
        assert_eq!(
            foreign_host_var("run ${CLAUDE_PLUGIN_ROOT}/scripts/x.sh"),
            Some("CLAUDE_PLUGIN_ROOT")
        );
    }

    #[test]
    fn flags_bare_foreign_var() {
        assert_eq!(
            foreign_host_var("cd $CLAUDE_PROJECT_DIR && go"),
            Some("CLAUDE_PROJECT_DIR")
        );
    }

    #[test]
    fn portable_body_passes() {
        assert_eq!(foreign_host_var("cd $HOME && cargo $1 --release"), None);
    }

    #[test]
    fn does_not_match_longer_identifier() {
        // `$CODEX_HOMELAB` must not be read as the foreign `CODEX_HOME`.
        assert_eq!(foreign_host_var("echo $CODEX_HOMELAB"), None);
    }
}
