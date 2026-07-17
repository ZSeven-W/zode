//! Stable per-agent avatar color, shared by the environment card's compact
//! Subagents summary row (`environment/row.rs`) and the M2 dedicated panel
//! (`subagents_panel.rs`) - both need the exact same color for a given
//! sub-agent id, or the panel's per-row dot would visibly disagree with the
//! summary strip that opened it.

use jian_widgets::Color;

/// Small, visually distinct palette a sub-agent's id is stably hashed into,
/// standing in for Codex's "colored avatar per agent" affordance. Chosen to
/// read clearly on both light and dark surfaces.
const SUBAGENT_PALETTE: [Color; 6] = [
    Color::rgb_u8(124, 58, 237), // violet
    Color::rgb_u8(37, 99, 235),  // blue
    Color::rgb_u8(5, 150, 105),  // emerald
    Color::rgb_u8(217, 119, 6),  // amber
    Color::rgb_u8(219, 39, 119), // pink
    Color::rgb_u8(8, 145, 178),  // cyan
];

/// Resolves one sub-agent id to its stable avatar color. Stable across
/// renders and across the agent's lifetime (the id never changes once the
/// registry allocates it), so one sub-agent keeps the same color from
/// `Running` through its terminal status, and across the summary row and
/// the dedicated panel alike.
pub(crate) fn subagent_avatar_color(id: &str) -> Color {
    SUBAGENT_PALETTE[stable_palette_index(id, SUBAGENT_PALETTE.len())]
}

/// FNV-1a over the id, reduced into a palette index.
fn stable_palette_index(id: &str, palette_len: usize) -> usize {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash as usize) % palette_len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_id_always_resolves_to_the_same_color() {
        assert_eq!(
            subagent_avatar_color("agent-1"),
            subagent_avatar_color("agent-1")
        );
    }

    #[test]
    fn index_never_exceeds_the_palette() {
        for id in ["a", "bb", "ccc", "subagent-42", ""] {
            assert!(stable_palette_index(id, SUBAGENT_PALETTE.len()) < SUBAGENT_PALETTE.len());
        }
    }
}
