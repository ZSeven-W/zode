use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    demo_state, AttachmentMetadata, FileArtifact, GoalProgress, TranscriptItem, TranscriptState,
};
use zode_app_ui::{SemanticIcon, ThreadTranscript, ZodeTheme};
use zode_node_protocol::SessionLocator;

#[derive(Debug, Clone, PartialEq)]
enum PaintOp {
    FillRound(Rect, f32, Color),
    StrokeRound(Rect, f32, Color, f32),
    Text(String, Point2D, Color),
    Svg(String, f32),
}

#[derive(Default)]
struct CapturePainter {
    operations: Vec<PaintOp>,
}

impl Painter for CapturePainter {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, origin: Point2D) {
        let text = layout
            .runs()
            .iter()
            .map(|run| run.content.as_str())
            .collect::<String>();
        let color = layout.runs().first().map_or(Color::TRANSPARENT, |run| {
            Color::rgba_u8(
                run.color.r(),
                run.color.g(),
                run.color.b(),
                f32::from(run.color.a()) / 255.0,
            )
        });
        self.operations.push(PaintOp::Text(text, origin, color));
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        self.operations
            .push(PaintOp::FillRound(rect, radius, color));
    }
    fn stroke_round_rect(&mut self, rect: Rect, radius: f32, color: Color, width: f32) {
        self.operations
            .push(PaintOp::StrokeRound(rect, radius, color, width));
    }
    fn stroke_svg_path(
        &mut self,
        d: &str,
        _top_left: Point2D,
        size: f32,
        _color: Color,
        _width: f32,
    ) {
        self.operations.push(PaintOp::Svg(d.to_owned(), size));
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
fn artifact_cards_use_compact_theme_adaptive_twelve_pixel_chrome() {
    for theme in [ZodeTheme::light(), ZodeTheme::dark()] {
        let painter = paint_artifacts(&theme);
        let cards = painter
            .operations
            .iter()
            .filter_map(|operation| match operation {
                PaintOp::FillRound(rect, radius, color)
                    if *color == theme.tokens.card && rect.size.x > 300.0 =>
                {
                    Some((*rect, *radius))
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(cards.len(), 4);
        assert!(cards
            .iter()
            .all(|(rect, radius)| rect.size.y == 64.0 && *radius == 12.0));
        assert_eq!(
            painter
                .operations
                .iter()
                .filter(|operation| matches!(
                    operation,
                    PaintOp::StrokeRound(_, 12.0, color, 1.0)
                        if *color == theme.tokens.border
                ))
                .count(),
            4
        );
    }
}

#[test]
fn artifact_card_hierarchy_aligns_text_and_uses_semantic_change_colors() {
    let theme = ZodeTheme::light();
    let painter = paint_artifacts(&theme);

    assert_text_color(&painter, "+12", theme.success);
    assert_text_color(&painter, "-4", theme.tokens.destructive);
    assert_text_color(&painter, "unparsed delta", theme.success);
    assert_text_color(&painter, "打开", theme.tokens.muted_foreground);
    assert!(!painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(rect, _, color)
            if rect.size.y <= 8.0 && *color == theme.zode_purple
    )));

    let cards = painter
        .operations
        .iter()
        .filter_map(|operation| match operation {
            PaintOp::FillRound(rect, 12.0, color)
                if *color == theme.tokens.card && rect.size.x > 300.0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    for title in [
        "Edited files",
        "Fallback change",
        "reference.png",
        "Ship polish",
    ] {
        let (_, origin, _) = text_op(&painter, title);
        let card = cards
            .iter()
            .find(|card| card.contains(origin))
            .expect("title should sit inside its artifact card");
        assert_eq!(origin.y - card.origin.y, 14.0, "misaligned title: {title}");
    }
    assert_eq!(text_op(&painter, "+12").1.y, text_op(&painter, "-4").1.y);
    assert_eq!(
        text_op(&painter, "reference.png").1.y,
        text_op(&painter, "打开").1.y
    );
    assert_eq!(
        text_op(&painter, "Ship polish").1.y,
        text_op(&painter, "3 / 7").1.y
    );
    let icon_paths = painter
        .operations
        .iter()
        .filter_map(|operation| match operation {
            PaintOp::Svg(path, 18.0) => Some(path.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(icon_paths.len(), 4);
    assert_eq!(
        icon_paths
            .iter()
            .filter(|path| **path == SemanticIcon::FileText.path())
            .count(),
        2
    );
    assert!(icon_paths.contains(&SemanticIcon::Snapshot.path()));
    assert!(icon_paths.contains(&SemanticIcon::Sparkles.path()));
}

fn paint_artifacts(theme: &ZodeTheme) -> CapturePainter {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "artifact-visuals");
    state.current_session = Some(session.clone());
    state.transcripts.insert(
        session,
        TranscriptState {
            items: vec![
                TranscriptItem::FileArtifact(FileArtifact {
                    id: "file-parsed".into(),
                    path: "crates/zode-app-ui/src/lib.rs".into(),
                    summary: "Edited files".into(),
                    change_summary: Some("+12 -4".into()),
                }),
                TranscriptItem::FileArtifact(FileArtifact {
                    id: "file-fallback".into(),
                    path: "assets/reference.bin".into(),
                    summary: "Fallback change".into(),
                    change_summary: Some("unparsed delta".into()),
                }),
                TranscriptItem::Attachment(AttachmentMetadata {
                    id: "attachment".into(),
                    path: Some("/tmp/reference.png".into()),
                    display_name: "reference.png".into(),
                    media_type: "image/png".into(),
                    width: Some(640),
                    height: Some(360),
                    byte_len: 8_192,
                }),
                TranscriptItem::GoalProgress(GoalProgress {
                    id: "goal".into(),
                    title: "Ship polish".into(),
                    completed: 3,
                    total: 7,
                }),
            ],
            follow_tail: false,
            ..TranscriptState::default()
        },
    );
    let mut painter = CapturePainter::default();
    ThreadTranscript::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 720.0, 500.0),
        &state,
        theme,
    );
    painter
}

fn assert_text_color(painter: &CapturePainter, text: &str, expected: Color) {
    assert_eq!(text_op(painter, text).2, expected, "wrong color for {text}");
}

fn text_op(painter: &CapturePainter, expected: &str) -> (String, Point2D, Color) {
    painter
        .operations
        .iter()
        .find_map(|operation| match operation {
            PaintOp::Text(text, origin, color) if text == expected => {
                Some((text.clone(), *origin, *color))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing text operation: {expected}"))
}
