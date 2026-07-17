use accesskit::{Action, Role};
use jian_core::CursorHint;
use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{demo_state, SettingsCategory, ShellRoute};
use zode_app_ui::{
    Insets, InteractionNode, RectExt, WidgetId, WorkspaceLayout, WorkspaceShell, WorkspaceSnapshot,
    ZodeTheme, CONTENT_W, SIDEBAR_W,
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

    fn rounded_rects(&self) -> impl Iterator<Item = (Rect, f32)> + '_ {
        self.operations
            .iter()
            .filter_map(|operation| match operation {
                PaintOp::FillRound(rect, radius, _) => Some((*rect, *radius)),
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
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

fn paint_empty_wide() -> (CapturePainter, WorkspaceLayout) {
    let viewport = Rect::xywh(0.0, 0.0, 1800.0, 1080.0);
    let geometry = WorkspaceLayout::compute(1800.0, 1080.0, Insets::ZERO);
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

#[test]
fn wide_shell_keeps_the_warm_rail_white_canvas_and_floating_composer_contract() {
    let viewport = Rect::xywh(0.0, 0.0, 1800.0, 1080.0);
    let geometry = WorkspaceLayout::compute(1800.0, 1080.0, Insets::ZERO);
    let theme = ZodeTheme::light();
    let mut painter = CapturePainter::default();

    WorkspaceShell::paint(&mut painter, viewport, Insets::ZERO, &demo_state(), &theme);

    assert_eq!(geometry.sidebar.width(), SIDEBAR_W);
    assert_eq!(geometry.composer.width(), CONTENT_W);
    assert_eq!(geometry.composer.max_y(), 1066.0);
    assert!(painter
        .operations
        .contains(&PaintOp::Fill(viewport, Color::WHITE)));
    assert!(painter
        .operations
        .contains(&PaintOp::Fill(geometry.sidebar, theme.sidebar)));
    assert!(theme.sidebar.r < theme.tokens.background.r);
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(rect, radius, color)
            if *rect == geometry.composer
                && (10.0..=16.0).contains(radius)
                && *color == theme.tokens.card
    )));
    assert!(painter.texts().contains(&"Zode"));
}

#[test]
fn snapshot_paint_uses_the_composer_interaction_rect() {
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
        }],
        focused: Some(composer_id),
    };
    let mut painter = CapturePainter::default();

    let painted =
        WorkspaceShell::paint_snapshot(&mut painter, &snapshot, &state, &ZodeTheme::light());

    assert_eq!(painted, snapshot.layout);
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(rect, _, _) if *rect == snapshot.nodes[0].rect
    )));
}

#[test]
fn empty_conversation_exposes_zode_guidance_and_full_composer_chrome() {
    let (painter, geometry) = paint_empty_wide();
    let text = painter.texts().join("\n");

    assert!(text.contains("我们在 Zode 中构建什么？"));
    for suggestion in ["探索代码", "构建功能", "审查变更", "修复问题"] {
        assert!(text.contains(suggestion));
    }
    for composer_chrome in ["zode", "本地", "完全访问"] {
        assert!(text.contains(composer_chrome));
    }
    assert!(!painter.texts().contains(&"main"));
    assert!(geometry.composer.min_y() > 900.0);
}

#[test]
fn empty_conversation_title_is_centered_in_the_main_surface() {
    let (painter, geometry) = paint_empty_wide();
    let title = "我们在 Zode 中构建什么？";
    let (origin, font_size) = painter.text(title).expect("empty-state title is painted");
    let title_center = origin.x + estimated_text_width(title, font_size) / 2.0;
    let main_center =
        geometry.sidebar.max_x() + (geometry.viewport.max_x() - geometry.sidebar.max_x()) / 2.0;

    assert!((title_center - main_center).abs() <= 32.0);
    assert!((380.0..=560.0).contains(&origin.y));
}

#[test]
fn empty_conversation_paints_four_distinct_suggestion_cards() {
    let (painter, geometry) = paint_empty_wide();
    let cards = painter
        .rounded_rects()
        .filter(|(rect, radius)| {
            rect.min_x() >= geometry.transcript.min_x() - 32.0
                && rect.max_x() <= geometry.transcript.max_x() + 32.0
                && (420.0..=800.0).contains(&rect.min_y())
                && (120.0..=220.0).contains(&rect.width())
                && (72.0..=144.0).contains(&rect.height())
                && (10.0..=16.0).contains(radius)
        })
        .collect::<Vec<_>>();

    assert_eq!(cards.len(), 4);
    for pair in cards.windows(2) {
        assert!(pair[0].0.max_x() < pair[1].0.min_x());
    }
}

#[test]
fn empty_conversation_uses_a_zode_mark_and_colored_suggestion_glyphs() {
    let (painter, geometry) = paint_empty_wide();
    let glyphs = painter
        .operations
        .iter()
        .filter_map(|operation| match operation {
            PaintOp::Svg {
                top_left,
                size,
                color,
            } if geometry.transcript.contains(*top_left) => Some((*top_left, *size, *color)),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(glyphs.len(), 6);
    assert!(glyphs.iter().any(|(_, size, _)| *size >= 36.0));
    let mut colors = Vec::new();
    for (_, _, color) in glyphs {
        if !colors.contains(&color) {
            colors.push(color);
        }
    }
    assert!(colors.len() >= 5);
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
    for label in ["常规", "本地运行状态", "主机连接", "活动工作区"] {
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
fn settings_route_does_not_paint_chat_transcript_or_composer() {
    let (painter, _) = paint_settings(SettingsCategory::General);
    let text = painter.texts().join("\n");

    assert!(!text.contains("向 Zode 描述一个任务"));
    assert!(!text.contains("选择模型"));
}
