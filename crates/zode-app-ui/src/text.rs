use jian_widgets::{Color, HorizontalAlign, Painter, Rect, TextBox, VerticalAlign};

pub(crate) fn paint_single_line(
    painter: &mut dyn Painter,
    text: &str,
    rect: Rect,
    size: f32,
    weight: u16,
    color: Color,
    horizontal_align: HorizontalAlign,
) {
    TextBox::new(text)
        .with_font_family("system-ui")
        .with_font_size(size)
        .with_font_weight(weight)
        .with_color(color)
        .with_horizontal_align(horizontal_align)
        .with_vertical_align(VerticalAlign::Center)
        .paint(painter, rect);
}
