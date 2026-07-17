use jian_widgets::{Painter, Point2D, Rect, TextLayout};
use zode_node_protocol::UsageSnapshot;

use crate::ZodeTheme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageDisplay {
    pub model: String,
    pub context: String,
    pub tokens: String,
    pub cost: String,
}

pub struct UsageChip;

impl UsageChip {
    pub fn display(model: Option<&str>, usage: &UsageSnapshot) -> UsageDisplay {
        UsageDisplay {
            model: model.unwrap_or("n/a").to_owned(),
            context: usage
                .context_used
                .map(|fraction| format!("{:.0}%", fraction.clamp(0.0, 1.0) * 100.0))
                .unwrap_or_else(|| "n/a".into()),
            tokens: format_integer(usage.input_tokens.saturating_add(usage.output_tokens)),
            cost: usage
                .cost_usd
                .map(|cost| format!("${cost:.4}"))
                .unwrap_or_else(|| "n/a".into()),
        }
    }

    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        model: Option<&str>,
        usage: &UsageSnapshot,
        theme: &ZodeTheme,
    ) {
        let display = Self::display(model, usage);
        painter.fill_round_rect(rect, rect.size.y / 2.0, theme.tokens.muted);
        let label = format!(
            "{} · {} · {} tok · {}",
            display.model, display.context, display.tokens, display.cost
        );
        let layout = TextLayout::single_run(
            &label,
            "system-ui",
            10.0,
            theme.tokens.muted_foreground.to_jian(),
            Point2D::ZERO,
        )
        .with_font_weight(500);
        painter.draw_text(
            &layout,
            Point2D::new(rect.origin.x + 10.0, rect.origin.y + rect.size.y * 0.68),
        );
    }
}

fn format_integer(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        let remaining = digits.len() - index;
        if index > 0 && matches!(remaining, 3 | 6 | 9 | 12 | 15 | 18) {
            output.push(',');
        }
        output.push(character);
    }
    output
}
