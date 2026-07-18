//! stdin JSON-lines wire protocol. zode-core/src/desktop/overlay.rs declares
//! the serialize side of the same shape; the golden tests below exist in BOTH
//! crates with identical fixture strings so the copies cannot drift.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pulse {
    Click,
    Scroll,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum OverlayCmd {
    Show {
        banner: String,
        esc_hint: String,
    },
    Move {
        x: f64,
        y: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window_id: Option<u32>,
        pulse: Pulse,
    },
    Chip {
        text: String,
    },
    Hide,
    Quit,
}

/// Parse one wire line. Unknown commands and malformed lines yield `None`
/// (forward compatibility: an older helper ignores newer commands).
pub fn parse_line(line: &str) -> Option<OverlayCmd> {
    serde_json::from_str(line.trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Golden wire fixtures — keep byte-identical with the copies in
    // zode-core/src/desktop/overlay.rs. ──
    const G_SHOW: &str = r#"{"cmd":"show","banner":"b","esc_hint":"e"}"#;
    const G_MOVE: &str = r#"{"cmd":"move","x":10.0,"y":20.5,"window_id":42,"pulse":"click"}"#;
    const G_MOVE_NOWIN: &str = r#"{"cmd":"move","x":1.0,"y":2.0,"pulse":"none"}"#;
    const G_CHIP: &str = r#"{"cmd":"chip","text":"⌨ Cmd+F"}"#;
    const G_HIDE: &str = r#"{"cmd":"hide"}"#;
    const G_QUIT: &str = r#"{"cmd":"quit"}"#;

    #[test]
    fn golden_lines_parse() {
        assert_eq!(
            parse_line(G_SHOW),
            Some(OverlayCmd::Show {
                banner: "b".into(),
                esc_hint: "e".into()
            })
        );
        assert_eq!(
            parse_line(G_MOVE),
            Some(OverlayCmd::Move {
                x: 10.0,
                y: 20.5,
                window_id: Some(42),
                pulse: Pulse::Click
            })
        );
        assert_eq!(
            parse_line(G_MOVE_NOWIN),
            Some(OverlayCmd::Move {
                x: 1.0,
                y: 2.0,
                window_id: None,
                pulse: Pulse::None
            })
        );
        assert_eq!(
            parse_line(G_CHIP),
            Some(OverlayCmd::Chip {
                text: "⌨ Cmd+F".into()
            })
        );
        assert_eq!(parse_line(G_HIDE), Some(OverlayCmd::Hide));
        assert_eq!(parse_line(G_QUIT), Some(OverlayCmd::Quit));
    }

    #[test]
    fn golden_lines_roundtrip_serialize() {
        // Serializing the parsed value must reproduce the fixture exactly —
        // this is what pins the zode-core writer copy to this parser.
        for g in [G_SHOW, G_MOVE, G_MOVE_NOWIN, G_CHIP, G_HIDE, G_QUIT] {
            let cmd = parse_line(g).unwrap();
            assert_eq!(serde_json::to_string(&cmd).unwrap(), g);
        }
    }

    #[test]
    fn unknown_or_malformed_is_none() {
        assert_eq!(parse_line(r#"{"cmd":"dance"}"#), None);
        assert_eq!(parse_line("not json"), None);
        assert_eq!(parse_line(""), None);
    }
}
