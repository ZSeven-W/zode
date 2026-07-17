use accesskit::{Action, Role};
use jian_core::CursorHint;
use jian_widgets::{Color, ImageDrawMode, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{demo_state, SettingsCategory, ShellRoute};
use zode_app_ui::{
    Composer, Insets, InteractionNode, RectExt, ThemeMode, WidgetId, WorkspaceLayout,
    WorkspaceShell, WorkspaceSnapshot, ZodeTheme, CONTENT_W, SIDEBAR_W, TRANSCRIPT_COMPOSER_GAP,
};

#[derive(Debug, Clone, PartialEq)]
enum PaintOp {
    Fill(Rect, Color),
    FillRound(Rect, f32, Color),
    StrokeRound(Rect, f32, Color, f32),
    Text {
        content: String,
        origin: Point2D,
        font_size: f32,
    },
    Svg {
        top_left: Point2D,
        size: f32,
        color: Color,
    },
    Image {
        rect: Rect,
        image_id: u64,
        encoded: Vec<u8>,
        mode: ImageDrawMode,
    },
    Clip(Rect),
    Save,
    Restore,
}

#[derive(Default)]
struct CapturePainter {
    operations: Vec<PaintOp>,
}

impl CapturePainter {
    fn texts(&self) -> Vec<&str> {
        self.operations
            .iter()
            .filter_map(|operation| match operation {
                PaintOp::Text { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect()
    }

    fn text(&self, expected: &str) -> Option<(Point2D, f32)> {
        self.operations
            .iter()
            .find_map(|operation| match operation {
                PaintOp::Text {
                    content,
                    origin,
                    font_size,
                } if content == expected => Some((*origin, *font_size)),
                _ => None,
            })
    }

    fn text_operations(&self) -> impl Iterator<Item = (&str, Point2D, f32)> + '_ {
        self.operations
            .iter()
            .filter_map(|operation| match operation {
                PaintOp::Text {
                    content,
                    origin,
                    font_size,
                } => Some((content.as_str(), *origin, *font_size)),
                _ => None,
            })
    }

    fn rounded_rects(&self) -> impl Iterator<Item = (Rect, f32)> + '_ {
        self.operations
            .iter()
            .filter_map(|operation| match operation {
                PaintOp::FillRound(rect, radius, _) => Some((*rect, *radius)),
                _ => None,
            })
    }

    fn images(&self) -> impl Iterator<Item = (Rect, u64, &[u8], ImageDrawMode)> + '_ {
        self.operations
            .iter()
            .filter_map(|operation| match operation {
                PaintOp::Image {
                    rect,
                    image_id,
                    encoded,
                    mode,
                } => Some((*rect, *image_id, encoded.as_slice(), *mode)),
                _ => None,
            })
    }
}

impl Painter for CapturePainter {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.operations.push(PaintOp::Fill(rect, color));
    }
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, origin: Point2D) {
        self.operations.push(PaintOp::Text {
            content: layout
                .runs()
                .iter()
                .map(|run| run.content.as_str())
                .collect(),
            origin,
            font_size: layout
                .runs()
                .first()
                .map(|run| run.font_size)
                .unwrap_or_default(),
        });
    }
    fn clip_rect(&mut self, rect: Rect) {
        self.operations.push(PaintOp::Clip(rect));
    }
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
        _d: &str,
        top_left: Point2D,
        size: f32,
        color: Color,
        _width: f32,
    ) {
        self.operations.push(PaintOp::Svg {
            top_left,
            size,
            color,
        });
    }
    fn draw_image_with_mode(
        &mut self,
        rect: Rect,
        image_id: u64,
        encoded: &[u8],
        mode: ImageDrawMode,
    ) {
        self.operations.push(PaintOp::Image {
            rect,
            image_id,
            encoded: encoded.to_vec(),
            mode,
        });
    }
    fn save(&mut self) {
        self.operations.push(PaintOp::Save);
    }
    fn restore(&mut self) {
        self.operations.push(PaintOp::Restore);
    }
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

fn paint_empty_wide_with_theme(theme: &ZodeTheme) -> (CapturePainter, WorkspaceLayout) {
    let viewport = Rect::xywh(0.0, 0.0, 1800.0, 1080.0);
    let geometry = WorkspaceLayout::compute(1800.0, 1080.0, Insets::ZERO);
    let mut painter = CapturePainter::default();
    WorkspaceShell::paint(&mut painter, viewport, Insets::ZERO, &demo_state(), theme);
    (painter, geometry)
}

fn paint_empty_wide() -> (CapturePainter, WorkspaceLayout) {
    paint_empty_wide_with_theme(&ZodeTheme::light())
}

fn paint_empty_compact() -> (CapturePainter, WorkspaceLayout) {
    let viewport = Rect::xywh(0.0, 0.0, 352.0, 480.0);
    let geometry = WorkspaceLayout::compute(352.0, 480.0, Insets::ZERO);
    let mut painter = CapturePainter::default();
    WorkspaceShell::paint(
        &mut painter,
        viewport,
        Insets::ZERO,
        &demo_state(),
        &ZodeTheme::light(),
    );
    (painter, geometry)
}

fn repository_asset(filename: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets")
            .join(filename),
    )
    .unwrap_or_else(|error| panic!("read repository brand asset {filename}: {error}"))
}

fn estimated_text_width(text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|character| {
            if character.is_ascii() {
                font_size * 0.55
            } else {
                font_size
            }
        })
        .sum()
}

fn assert_close(actual: f32, expected: f32, tolerance: f32, label: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{label}: expected {expected} ± {tolerance}, got {actual}"
    );
}

fn assert_rect_close(actual: Rect, expected: Rect, tolerance: f32, label: &str) {
    assert_close(
        actual.min_x(),
        expected.min_x(),
        tolerance,
        &format!("{label}.x"),
    );
    assert_close(
        actual.min_y(),
        expected.min_y(),
        tolerance,
        &format!("{label}.y"),
    );
    assert_close(
        actual.width(),
        expected.width(),
        tolerance,
        &format!("{label}.width"),
    );
    assert_close(
        actual.height(),
        expected.height(),
        tolerance,
        &format!("{label}.height"),
    );
}

fn contained_by(inner: Rect, outer: Rect) -> bool {
    inner.min_x() >= outer.min_x()
        && inner.min_y() >= outer.min_y()
        && inner.max_x() <= outer.max_x()
        && inner.max_y() <= outer.max_y()
}

fn overlaps(left: Rect, right: Rect) -> bool {
    left.min_x() < right.max_x()
        && left.max_x() > right.min_x()
        && left.min_y() < right.max_y()
        && left.max_y() > right.min_y()
}

#[test]
fn wide_shell_keeps_the_frosted_rail_white_canvas_and_floating_composer_contract() {
    let viewport = Rect::xywh(0.0, 0.0, 1800.0, 1080.0);
    let geometry = WorkspaceLayout::compute(1800.0, 1080.0, Insets::ZERO);
    let theme = ZodeTheme::light();
    let mut painter = CapturePainter::default();
    let state = demo_state();

    WorkspaceShell::paint(&mut painter, viewport, Insets::ZERO, &state, &theme);
    let composer = Composer::layout(geometry.composer, &state.composer);

    assert_eq!(geometry.sidebar.width(), SIDEBAR_W);
    assert_eq!(geometry.composer.width(), CONTENT_W);
    assert_eq!(geometry.composer.max_y(), 1066.0);
    assert!(painter
        .operations
        .contains(&PaintOp::Fill(viewport, Color::WHITE)));
    assert!(painter
        .operations
        .contains(&PaintOp::Fill(geometry.sidebar, theme.sidebar)));
    assert_eq!(theme.sidebar, Color::rgb_u8(245, 246, 246));
    assert!(theme.sidebar.r < theme.tokens.background.r);
    let edge_bands = painter
        .operations
        .iter()
        .filter(|operation| {
            matches!(
                operation,
                PaintOp::Fill(rect, color)
                    if rect.min_x() >= geometry.sidebar.max_x() - 8.0
                        && rect.max_x() <= geometry.sidebar.max_x()
                        && rect.height() == geometry.sidebar.height()
                        && color.a < 1.0
            )
        })
        .count();
    assert_eq!(edge_bands, 8, "sidebar material keeps its subtle edge ramp");
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(rect, radius, color)
            if *rect == composer.input
                && (10.0..=16.0).contains(radius)
                && *color == theme.tokens.card
    )));
    assert!(painter.texts().contains(&"Zode"));
}

#[test]
fn snapshot_paint_uses_the_full_stack_not_an_input_node_override() {
    let state = demo_state();
    let layout = WorkspaceLayout::compute(1221.0, 992.0, Insets::ZERO);
    let composer_id = WidgetId(20);
    let composer_rect = Rect::xywh(
        layout.composer.min_x() + 7.0,
        layout.composer.min_y() + 11.0,
        layout.composer.width() - 14.0,
        layout.composer.height() - 22.0,
    );
    let snapshot = WorkspaceSnapshot {
        layout,
        nodes: vec![InteractionNode {
            id: composer_id,
            rect: composer_rect,
            role: Role::TextInput,
            name: "要求后续变更".into(),
            value: Some(String::new()),
            actions: vec![Action::Focus, Action::SetValue],
            focus_order: Some(0),
            cursor: CursorHint::Text,
            toggled: None,
            disabled: false,
        }],
        focused: Some(composer_id),
    };
    let mut painter = CapturePainter::default();

    let painted =
        WorkspaceShell::paint_snapshot(&mut painter, &snapshot, &state, &ZodeTheme::light());
    let composer = Composer::layout(snapshot.layout.composer, &state.composer);

    assert_eq!(painted, snapshot.layout);
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(rect, _, _) if *rect == composer.input
    )));
    assert!(!painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(rect, _, _) if *rect == snapshot.nodes[0].rect
    )));
}

#[test]
fn empty_conversation_exposes_zode_guidance_and_full_composer_chrome() {
    let (painter, geometry) = paint_empty_wide();
    let text = painter.texts().concat();

    assert!(text.contains("我们应该构建什么？"));
    for suggestion in [
        "探索并理解代码",
        "构建新功能、应用或工具",
        "审查代码并提出修改建议",
        "修复问题和失败",
    ] {
        assert!(text.contains(suggestion));
    }
    for superseded_copy in [
        "理解现有项目",
        "实现应用或工具",
        "检查代码并提出建议",
        "诊断失败并修复",
    ] {
        assert!(!text.contains(superseded_copy));
    }
    assert!(text.contains("本地"));
    assert!(!painter.texts().contains(&"新任务"));
    assert!(!painter.texts().contains(&"zode"));
    assert!(!painter.texts().contains(&"main"));
    assert!(!painter.texts().contains(&"完全访问"));
    assert!(geometry.composer.min_y() > 900.0);
}

#[test]
fn empty_conversation_title_is_centered_in_the_main_surface() {
    let (painter, geometry) = paint_empty_wide();
    let title = "我们应该构建什么？";
    let (origin, font_size) = painter.text(title).expect("empty-state title is painted");
    let title_center = origin.x + estimated_text_width(title, font_size) / 2.0;
    let main_center = geometry.transcript.min_x() + geometry.transcript.width() / 2.0;

    assert_close(title_center, main_center, 2.0, "empty title center x");
    assert_close(origin.y, 501.0, 2.0, "empty title top y");
}

#[test]
fn empty_conversation_paints_four_distinct_suggestion_cards() {
    let (painter, geometry) = paint_empty_wide();
    let cards = painter
        .rounded_rects()
        .filter(|(rect, radius)| {
            rect.min_x() >= geometry.transcript.min_x()
                && rect.max_x() <= geometry.transcript.max_x()
                && (540.0..=700.0).contains(&rect.min_y())
                && (150.0..=190.0).contains(&rect.width())
                && (90.0..=120.0).contains(&rect.height())
                && (10.0..=16.0).contains(radius)
        })
        .collect::<Vec<_>>();

    assert_eq!(cards.len(), 4);
    for (index, (card, _)) in cards.iter().enumerate() {
        let expected = Rect::xywh(664.0 + index as f32 * 180.5, 563.0, 170.5, 106.0);
        assert_rect_close(*card, expected, 2.0, &format!("suggestion card {index}"));
    }
    for (index, pair) in cards.windows(2).enumerate() {
        assert_close(
            pair[1].0.min_x() - pair[0].0.max_x(),
            10.0,
            1.0,
            &format!("suggestion card gap {index}"),
        );
    }
}

#[test]
fn empty_conversation_uses_the_theme_specific_repository_brand_mark() {
    const DARK_IMAGE_ID: u64 = 0x248d_4a5b_143e_0780;
    const LIGHT_IMAGE_ID: u64 = 0x69f3_60d5_c3e4_1b23;

    for (theme, filename, expected_image_id) in [
        (ZodeTheme::light(), "logo-light.png", LIGHT_IMAGE_ID),
        (ZodeTheme::dark(), "logo.png", DARK_IMAGE_ID),
        (
            ZodeTheme::high_contrast(ThemeMode::Light),
            "logo-light.png",
            LIGHT_IMAGE_ID,
        ),
        (
            ZodeTheme::high_contrast(ThemeMode::Dark),
            "logo.png",
            DARK_IMAGE_ID,
        ),
    ] {
        let (painter, geometry) = paint_empty_wide_with_theme(&theme);
        let images = painter.images().collect::<Vec<_>>();

        assert_eq!(images.len(), 1, "theme should paint one brand image");
        let (rect, image_id, encoded, mode) = images[0];
        assert_eq!(rect.size, Point2D::new(48.0, 48.0));
        assert_eq!(image_id, expected_image_id);
        assert_eq!(encoded, repository_asset(filename));
        assert_eq!(mode, ImageDrawMode::Fit);
        assert_rect_close(
            rect,
            Rect::xywh(996.0, 418.0, 48.0, 48.0),
            2.0,
            "empty brand mark",
        );

        let image_center = rect.min_x() + rect.width() / 2.0;
        let main_center = geometry.transcript.min_x() + geometry.transcript.width() / 2.0;
        assert_close(image_center, main_center, 2.0, "empty brand center x");
    }
}

#[test]
fn empty_conversation_only_uses_svg_paths_for_the_four_suggestions() {
    let (painter, geometry) = paint_empty_wide();
    let suggestion_glyphs = painter
        .operations
        .iter()
        .filter(|operation| {
            matches!(
                operation,
                PaintOp::Svg { top_left, .. } if geometry.transcript.contains(*top_left)
            )
        })
        .count();

    assert_eq!(suggestion_glyphs, 4);
}

#[test]
fn wide_suggestion_copy_wraps_without_shrinking_below_the_reference_size() {
    let (painter, geometry) = paint_empty_wide();
    let cards = painter
        .rounded_rects()
        .filter(|(rect, _)| {
            contained_by(*rect, geometry.transcript)
                && (150.0..=190.0).contains(&rect.width())
                && (90.0..=120.0).contains(&rect.height())
        })
        .map(|(rect, _)| rect)
        .collect::<Vec<_>>();
    assert_eq!(cards.len(), 4);

    for (index, (expected, expected_y)) in [
        ("探索并理解代码", &[641.0][..]),
        ("构建新功能、应用或工具", &[619.0, 637.0][..]),
        ("审查代码并提出修改建议", &[619.0, 637.0][..]),
        ("修复问题和失败", &[641.0][..]),
    ]
    .into_iter()
    .enumerate()
    {
        let lines = painter
            .text_operations()
            .filter(|(_, origin, _)| cards[index].contains(*origin))
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), expected_y.len(), "card {index} line count");
        assert_eq!(
            lines
                .iter()
                .map(|(content, _, _)| *content)
                .collect::<String>(),
            expected,
        );
        for ((content, origin, font_size), expected_y) in lines.into_iter().zip(expected_y) {
            assert!(font_size >= 12.9, "card {index} shrank to {font_size}px");
            assert_close(origin.y, *expected_y, 2.0, &format!("card {index} text y"));
            assert!(
                estimated_text_width(content, font_size) <= cards[index].width() - 28.0 + 0.5,
                "card {index} line exceeds its content width"
            );
        }
    }
}

#[test]
fn compact_empty_state_uses_a_clipped_readable_two_by_two_grid() {
    let (painter, geometry) = paint_empty_compact();
    assert_eq!(geometry.transcript.size, Point2D::new(320.0, 224.0));
    let composer = Composer::layout(geometry.composer, &demo_state().composer);
    let empty_viewport = Rect::xywh(
        geometry.transcript.min_x(),
        geometry.transcript.min_y(),
        geometry.transcript.width(),
        composer.input.min_y() - TRANSCRIPT_COMPOSER_GAP - geometry.transcript.min_y(),
    );
    assert_eq!(empty_viewport.size, Point2D::new(320.0, 268.0));
    let cards = painter
        .rounded_rects()
        .filter(|(rect, _)| {
            contained_by(*rect, empty_viewport) && rect.width() > 80.0 && rect.height() > 60.0
        })
        .map(|(rect, _)| rect)
        .collect::<Vec<_>>();
    assert_eq!(cards.len(), 4);
    assert_close(cards[0].min_y(), cards[1].min_y(), 0.1, "compact row 1");
    assert_close(cards[2].min_y(), cards[3].min_y(), 0.1, "compact row 2");
    assert_close(
        cards[1].min_x() - cards[0].max_x(),
        10.0,
        0.1,
        "compact x gap",
    );
    assert_close(
        cards[2].min_y() - cards[0].max_y(),
        10.0,
        0.1,
        "compact y gap",
    );
    for (index, card) in cards.iter().enumerate() {
        assert!(contained_by(*card, empty_viewport));
        for other in cards.iter().skip(index + 1) {
            assert!(!overlaps(*card, *other));
        }
        let text = painter
            .text_operations()
            .filter(|(_, origin, _)| card.contains(*origin))
            .collect::<Vec<_>>();
        assert!(!text.is_empty(), "card {index} keeps readable copy");
        assert!(text.iter().all(|(_, _, size)| *size >= 10.0));
        assert!(
            painter.operations.contains(&PaintOp::Clip(*card)),
            "card {index} is clipped"
        );
    }
    assert_eq!(
        painter
            .images()
            .filter(|(rect, _, _, _)| contained_by(*rect, geometry.transcript))
            .count(),
        0,
        "compact layout hides the brand mark",
    );
    assert!(
        painter
            .operations
            .iter()
            .filter(|operation| matches!(operation, PaintOp::Save))
            .count()
            >= 4
    );
    assert!(
        painter
            .operations
            .iter()
            .filter(|operation| matches!(operation, PaintOp::Restore))
            .count()
            >= 4
    );
}

fn paint_settings(category: SettingsCategory) -> (CapturePainter, WorkspaceLayout) {
    let mut state = demo_state();
    state.presentation.route = ShellRoute::Settings(category);
    let geometry = WorkspaceLayout::compute(1800.0, 1080.0, Insets::ZERO);
    let mut painter = CapturePainter::default();
    WorkspaceShell::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 1800.0, 1080.0),
        Insets::ZERO,
        &state,
        &ZodeTheme::light(),
    );
    (painter, geometry)
}

#[test]
fn settings_routes_expose_real_local_categories_without_cloud_login() {
    let (appearance, _) = paint_settings(SettingsCategory::Appearance);
    let appearance = appearance.texts().join("\n");

    for label in [
        "设置",
        "外观",
        "跟随系统",
        "浅色",
        "深色",
        "减少动画",
        "高对比度",
    ] {
        assert!(
            appearance.contains(label),
            "missing appearance label: {label}"
        );
    }
    assert!(!appearance.contains("项目权限"));
    assert!(!appearance.contains("登录"));

    let (general, _) = paint_settings(SettingsCategory::General);
    let general = general.texts().join("\n");
    for label in ["常规", "权限", "只读", "工作区写入", "默认文件打开目标"] {
        assert!(general.contains(label), "missing general label: {label}");
    }
    assert!(!general.contains("登录"));
}

#[test]
fn settings_route_has_a_category_rail_and_centered_grouped_card() {
    let (painter, geometry) = paint_settings(SettingsCategory::General);
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::Fill(rect, _) if *rect == geometry.sidebar
    )));

    let cards = painter
        .rounded_rects()
        .filter(|(rect, radius)| {
            rect.min_x() >= 400.0
                && rect.max_x() <= 1600.0
                && rect.min_y() >= 100.0
                && rect.max_y() <= 900.0
                && (600.0..=900.0).contains(&rect.width())
                && (80.0..=360.0).contains(&rect.height())
                && (10.0..=16.0).contains(radius)
        })
        .collect::<Vec<_>>();

    assert!(!cards.is_empty(), "settings has a grouped card");
    let main_center =
        geometry.sidebar.max_x() + (geometry.viewport.max_x() - geometry.sidebar.max_x()) / 2.0;
    for (card, _) in cards {
        assert!((card.min_x() + card.width() / 2.0 - main_center).abs() <= 48.0);
    }
}

#[test]
fn settings_general_paints_reference_heading_and_card_verticals() {
    let (painter, _) = paint_settings(SettingsCategory::General);
    let text_origin = |label: &str, size: f32| {
        painter
            .text_operations()
            .find(|(content, origin, font_size)| {
                *content == label && origin.x >= 600.0 && (*font_size - size).abs() <= 0.01
            })
            .map(|(_, origin, _)| origin)
            .unwrap_or_else(|| panic!("missing settings text: {label}/{size}"))
    };

    assert_close(text_origin("常规", 24.0).y, 70.0, 2.0, "settings title top");
    assert_close(
        text_origin("权限", 13.0).y,
        142.0,
        2.0,
        "permission label top",
    );
    assert_close(text_origin("常规", 13.0).y, 442.0, 2.0, "general label top");

    let cards = painter
        .rounded_rects()
        .map(|(rect, _)| rect)
        .filter(|rect| rect.origin.x == 636.0 && rect.size.x == 768.0)
        .collect::<Vec<_>>();
    assert!(cards.contains(&Rect::xywh(636.0, 174.0, 768.0, 216.0)));
    assert!(cards.contains(&Rect::xywh(636.0, 474.0, 768.0, 600.0)));
}

#[test]
fn settings_route_does_not_paint_chat_transcript_or_composer() {
    let (painter, _) = paint_settings(SettingsCategory::General);
    let text = painter.texts().join("\n");

    assert!(!text.contains("向 Zode 描述一个任务"));
    assert!(!text.contains("选择模型"));
}
