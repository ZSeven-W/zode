use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::demo_state;
use zode_app_ui::{ComposerFooterMenuWidget, Elevation, EmptyState, TypographyRole, ZodeTheme};

#[derive(Debug, Clone, Copy, PartialEq)]
struct ShadowCall {
    rect: Rect,
    radius: f32,
    blur: f32,
    color: Color,
}

#[derive(Debug, Clone, PartialEq)]
struct TextCall {
    content: String,
    size: f32,
    weight: u16,
}

#[derive(Default)]
struct CapturePainter {
    shadows: Vec<ShadowCall>,
    texts: Vec<TextCall>,
}

impl Painter for CapturePainter {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, _origin: Point2D) {
        let run = layout.runs().first().expect("single-run empty-state text");
        self.texts.push(TextCall {
            content: layout
                .runs()
                .iter()
                .map(|run| run.content.as_str())
                .collect(),
            size: run.font_size,
            weight: run.font_weight,
        });
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color) {}
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
    fn stroke_svg_path(
        &mut self,
        _d: &str,
        _top_left: Point2D,
        _size: f32,
        _color: Color,
        _width: f32,
    ) {
    }
    fn fill_drop_shadow(&mut self, rect: Rect, radius: f32, blur: f32, color: Color) {
        self.shadows.push(ShadowCall {
            rect,
            radius,
            blur,
            color,
        });
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

#[test]
fn wide_empty_state_keeps_reference_typography_and_card_depth() {
    let bounds = Rect::xywh(652.0, 70.0, 736.0, 868.0);
    let cards = EmptyState::suggestion_layouts(bounds);
    let mut painter = CapturePainter::default();

    EmptyState::paint_with_workspace(
        &mut painter,
        bounds,
        Some("zode"),
        false,
        false,
        &ZodeTheme::light(),
    );

    for text in ["我们应该在 ", "zode", " 中构建什么？"] {
        let call = painter
            .texts
            .iter()
            .find(|call| call.content == text)
            .unwrap_or_else(|| panic!("missing welcome title run: {text}"));
        assert_eq!(call.size, 27.0);
        assert_eq!(call.weight, 400);
    }
    for suggestion in cards {
        let call = painter
            .texts
            .iter()
            .find(|call| suggestion.label.contains(call.content.as_str()))
            .unwrap_or_else(|| panic!("missing suggestion text: {}", suggestion.label));
        assert_eq!(call.size, 13.0);
        assert_eq!(call.weight, 400);
    }

    // Each card now paints the shared two-layer `Elevation::prominent`
    // shadow (`paint_elevated_surface`) instead of a single hand-rolled
    // drop shadow with its own offset/blur/alpha — the rect is the card's
    // true position (no manual +2px offset baked into the call site
    // anymore) and the two layers match the shared token exactly.
    let elevation = Elevation::prominent();
    assert_eq!(painter.shadows.len(), cards.len() * 2);
    for (layers, card) in painter.shadows.chunks_exact(2).zip(cards) {
        let [shadow, glow] = layers else {
            unreachable!("chunks_exact(2) always yields two elements")
        };
        for layer in [shadow, glow] {
            assert_eq!(layer.rect.origin.x, card.rect.origin.x);
            assert_eq!(layer.rect.origin.y, card.rect.origin.y);
            assert_eq!(layer.rect.size, card.rect.size);
            assert_eq!(layer.radius, 12.0);
        }
        assert_eq!(shadow.blur, elevation.shadow.blur);
        assert_eq!(shadow.color, elevation.shadow.color);
        assert_eq!(glow.blur, elevation.glow.blur);
        assert_eq!(glow.color, elevation.glow.color);
    }
}

#[test]
fn composer_footer_and_transcript_status_use_reference_typography() {
    let mut state = demo_state();
    state.composer.sandbox_label = "完全访问".into();
    state.composer.model = Some("5.6 Sol".into());
    let input = Rect::xywh(300.0, 760.0, 736.0, 100.0);
    let layout = ComposerFooterMenuWidget::trigger_layout(input, &state);
    let mut painter = CapturePainter::default();

    ComposerFooterMenuWidget::paint_controls(
        &mut painter,
        layout,
        &state,
        false,
        false,
        None,
        None,
        &ZodeTheme::light(),
    );

    for label in ["完全访问", "5.6 Sol"] {
        let call = painter
            .texts
            .iter()
            .find(|call| call.content == label)
            .unwrap_or_else(|| panic!("missing composer footer label: {label}"));
        assert_eq!(call.size, 14.0);
        assert_eq!(call.weight, 400);
    }
    assert!(layout.permission.size.x >= 94.0);
    assert_eq!(TypographyRole::UiLabel.style().weight, 400);
}
