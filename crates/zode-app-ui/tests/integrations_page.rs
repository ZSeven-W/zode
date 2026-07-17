use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{demo_state, AppCommand, IntegrationsTab, ShellRoute};
use zode_app_ui::{IntegrationsPage, ZodeTheme};
use zode_node_protocol::NodeCapability;

#[derive(Debug, Clone)]
struct TextDraw {
    content: String,
    origin: Point2D,
    font_size: f32,
    weight: u16,
}

#[derive(Default)]
struct TextCapture {
    texts: Vec<String>,
    text_draws: Vec<TextDraw>,
    rounded_fills: Vec<Rect>,
    clips: Vec<Rect>,
    svg_strokes: usize,
    measurements: Vec<(String, f32, u16)>,
}

impl Painter for TextCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, origin: Point2D) {
        let content = layout
            .runs()
            .iter()
            .map(|run| run.content.as_str())
            .collect::<String>();
        let (font_size, weight) = layout
            .runs()
            .first()
            .map_or((0.0, 400), |run| (run.font_size, run.font_weight));
        self.texts.push(content.clone());
        self.text_draws.push(TextDraw {
            content,
            origin,
            font_size,
            weight,
        });
    }
    fn clip_rect(&mut self, rect: Rect) {
        self.clips.push(rect);
    }
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, _radius: f32, _color: Color) {
        self.rounded_fills.push(rect);
    }
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
    fn stroke_svg_path(
        &mut self,
        _d: &str,
        _top_left: Point2D,
        _size: f32,
        _color: Color,
        _width: f32,
    ) {
        self.svg_strokes += 1;
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
    fn measure_text_weighted(&mut self, text: &str, font_size: f32, weight: u16) -> f32 {
        self.measurements
            .push((text.to_string(), font_size, weight));
        estimated_text_width(text, font_size)
    }
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
fn wide_page_centers_a_736px_content_column_and_exposes_real_tab_commands() {
    let mut state = demo_state();
    state.presentation.route = ShellRoute::Integrations(IntegrationsTab::Plugins);
    let surface = Rect::xywh(240.0, 0.0, 1_560.0, 1_080.0);

    let layout = IntegrationsPage::layout(surface, &state);

    assert_eq!(layout.content, Rect::xywh(652.0, 46.0, 736.0, 1_034.0));
    assert_eq!(layout.search, Rect::xywh(652.0, 146.0, 736.0, 34.0));
    assert_eq!(layout.tabs[0].label, "插件");
    assert_eq!(layout.tabs[0].tab, IntegrationsTab::Plugins);
    assert!(layout.tabs[0].selected);
    assert_eq!(layout.tabs[1].label, "技能");
    assert_eq!(layout.tabs[1].tab, IntegrationsTab::Skills);
    assert!(!layout.tabs[1].selected);
    assert_eq!(layout.tabs[0].rect, Rect::xywh(248.0, 6.0, 48.0, 32.0));
    assert_eq!(layout.tabs[1].rect, Rect::xywh(300.0, 6.0, 48.0, 32.0));
    assert_eq!(
        IntegrationsPage::command_for_widget(layout.tabs[1].id),
        Some(AppCommand::SelectIntegrationsTab(IntegrationsTab::Skills)),
    );
}

#[test]
fn capability_cards_are_a_strict_projection_of_the_host_manifest() {
    let mut state = demo_state();
    state.host.capabilities.capabilities.extend([
        NodeCapability::Agent,
        NodeCapability::Browser,
        NodeCapability::Notifications,
    ]);

    let cards = IntegrationsPage::capability_cards(&state);

    assert_eq!(
        cards
            .iter()
            .map(|card| (card.capability.clone(), card.label, card.description))
            .collect::<Vec<_>>(),
        vec![
            (NodeCapability::Agent, "智能体", "运行并协调 AI 编码任务",),
            (NodeCapability::Browser, "浏览器", "打开网页并与页面交互",),
            (NodeCapability::Notifications, "通知", "发送本地系统通知",),
        ],
    );
}

#[test]
fn plugin_capabilities_use_a_stable_two_column_card_grid() {
    let mut state = demo_state();
    state.presentation.route = ShellRoute::Integrations(IntegrationsTab::Plugins);
    state.host.capabilities.capabilities.extend([
        NodeCapability::Agent,
        NodeCapability::Workspace,
        NodeCapability::Terminal,
    ]);
    let surface = Rect::xywh(240.0, 0.0, 1_560.0, 1_080.0);

    let cards = IntegrationsPage::capability_card_layout(surface, &state);

    assert_eq!(cards.len(), 3);
    assert_eq!(cards[0].rect, Rect::xywh(652.0, 252.0, 352.0, 74.0));
    assert_eq!(cards[1].rect, Rect::xywh(1_036.0, 252.0, 352.0, 74.0));
    assert_eq!(cards[2].rect, Rect::xywh(652.0, 338.0, 352.0, 74.0));
    assert_eq!(cards[0].card.capability, NodeCapability::Agent);
    assert_eq!(cards[1].card.capability, NodeCapability::Workspace);
    assert_eq!(cards[2].card.capability, NodeCapability::Terminal);
}

#[test]
fn narrow_plugins_use_one_clipped_column_with_a_14px_icon_inset() {
    let mut state = demo_state();
    state.presentation.route = ShellRoute::Integrations(IntegrationsTab::Plugins);
    state.host.capabilities.capabilities.extend([
        NodeCapability::Agent,
        NodeCapability::Workspace,
        NodeCapability::Terminal,
    ]);
    let surface = Rect::xywh(0.0, 0.0, 520.0, 720.0);

    let cards = IntegrationsPage::capability_card_layout(surface, &state);

    assert_eq!(cards.len(), 3);
    assert_eq!(cards[0].rect, Rect::xywh(0.0, 252.0, 520.0, 74.0));
    assert_eq!(cards[1].rect, Rect::xywh(0.0, 338.0, 520.0, 74.0));
    assert_eq!(cards[2].rect, Rect::xywh(0.0, 424.0, 520.0, 74.0));

    let mut painter = TextCapture::default();
    IntegrationsPage::paint(&mut painter, surface, &state, &ZodeTheme::light());

    for card in &cards {
        assert!(painter.clips.contains(&card.rect));
        let icon = painter
            .rounded_fills
            .iter()
            .find(|rect| {
                (rect.origin.x - (card.rect.origin.x + 14.0)).abs() <= 0.01
                    && rect.size == Point2D::new(40.0, 40.0)
                    && rect.origin.y >= card.rect.origin.y
                    && rect.origin.y + rect.size.y <= card.rect.origin.y + card.rect.size.y
            })
            .expect("each card keeps the 14px icon inset");
        assert!(
            (icon.origin.y + icon.size.y / 2.0 - (card.rect.origin.y + card.rect.size.y / 2.0))
                .abs()
                <= 0.01
        );
    }
}

#[test]
fn narrow_plugins_fit_all_eight_capabilities_and_center_compact_card_contents() {
    let mut state = demo_state();
    state.presentation.route = ShellRoute::Integrations(IntegrationsTab::Plugins);
    state.host.capabilities.capabilities.extend([
        NodeCapability::Agent,
        NodeCapability::Workspace,
        NodeCapability::FileSystem,
        NodeCapability::Terminal,
        NodeCapability::Browser,
        NodeCapability::Camera,
        NodeCapability::Notifications,
        NodeCapability::Approval,
    ]);
    let surface = Rect::xywh(0.0, 0.0, 520.0, 720.0);

    let cards = IntegrationsPage::capability_card_layout(surface, &state);

    assert_eq!(cards.len(), 8);
    assert!(cards[0].rect.size.y >= 48.0);
    assert!(cards[0].rect.size.y < 74.0);
    for pair in cards.windows(2) {
        let previous_bottom = pair[0].rect.origin.y + pair[0].rect.size.y;
        assert!(previous_bottom <= pair[1].rect.origin.y);
    }
    for card in &cards {
        assert!(card.rect.origin.y >= surface.origin.y);
        assert!(card.rect.origin.y + card.rect.size.y <= surface.origin.y + surface.size.y + 0.01);
    }

    let mut painter = TextCapture::default();
    IntegrationsPage::paint(&mut painter, surface, &state, &ZodeTheme::light());

    for card in &cards {
        let icon = painter
            .rounded_fills
            .iter()
            .find(|rect| {
                (rect.origin.x - (card.rect.origin.x + 14.0)).abs() <= 0.01
                    && rect.size.x <= 40.0
                    && rect.size.y <= 40.0
                    && rect.origin.y >= card.rect.origin.y
                    && rect.origin.y + rect.size.y <= card.rect.origin.y + card.rect.size.y + 0.01
            })
            .expect("each compact card paints a contained icon");
        let icon_center = icon.origin.y + icon.size.y / 2.0;
        let card_center = card.rect.origin.y + card.rect.size.y / 2.0;
        assert!((icon_center - card_center).abs() <= 0.01);

        for content in [card.card.label, card.card.description] {
            let draw = painter
                .text_draws
                .iter()
                .find(|draw| draw.content == content)
                .expect("each compact card paints both text rows");
            assert!(draw.origin.y > card.rect.origin.y);
            assert!(draw.origin.y < card.rect.origin.y + card.rect.size.y);
        }
    }
}

#[test]
fn plugin_paint_shows_only_real_node_capabilities_without_marketplace_claims() {
    let mut state = demo_state();
    state.presentation.route = ShellRoute::Integrations(IntegrationsTab::Plugins);
    state
        .host
        .capabilities
        .capabilities
        .extend([NodeCapability::Terminal, NodeCapability::Approval]);
    let mut painter = TextCapture::default();

    IntegrationsPage::paint(
        &mut painter,
        Rect::xywh(240.0, 0.0, 1_560.0, 1_080.0),
        &state,
        &ZodeTheme::light(),
    );

    let text = painter.texts.join("\n");
    for expected in [
        "插件",
        "技能",
        "使用当前节点提供的本地能力",
        "搜索即将支持",
        "可用能力",
        "终端",
        "运行本地命令与开发工具",
        "审批",
        "在敏感操作前请求确认",
    ] {
        assert!(text.contains(expected), "missing paint text: {expected}");
    }
    for fabricated in ["智能体", "已安装", "安装", "51 完成"] {
        assert!(
            !text.contains(fabricated),
            "fabricated marketplace state was painted: {fabricated}"
        );
    }
}

#[test]
fn skills_paint_is_an_explicit_empty_catalog_and_never_rebrands_capabilities() {
    let mut state = demo_state();
    state.presentation.route = ShellRoute::Integrations(IntegrationsTab::Skills);
    state
        .host
        .capabilities
        .capabilities
        .extend([NodeCapability::Agent, NodeCapability::Terminal]);
    let mut painter = TextCapture::default();

    IntegrationsPage::paint(
        &mut painter,
        Rect::xywh(240.0, 0.0, 1_560.0, 1_080.0),
        &state,
        &ZodeTheme::light(),
    );

    let text = painter.texts.join("\n");
    for expected in [
        "插件",
        "技能",
        "通过可复用指令扩展 Zode 工作流",
        "搜索即将支持",
        "尚未接入技能目录",
    ] {
        assert!(text.contains(expected), "missing paint text: {expected}");
    }
    for fabricated in ["智能体", "终端", "已安装", "安装"] {
        assert!(
            !text.contains(fabricated),
            "capability or marketplace state was painted as a skill: {fabricated}"
        );
    }
    assert!(IntegrationsPage::capability_card_layout(
        Rect::xywh(240.0, 0.0, 1_560.0, 1_080.0),
        &state,
    )
    .is_empty());
}

#[test]
fn search_shell_is_explicitly_unavailable_instead_of_looking_editable() {
    for tab in [IntegrationsTab::Plugins, IntegrationsTab::Skills] {
        let mut state = demo_state();
        state.presentation.route = ShellRoute::Integrations(tab);
        let mut painter = TextCapture::default();

        IntegrationsPage::paint(
            &mut painter,
            Rect::xywh(0.0, 0.0, 736.0, 720.0),
            &state,
            &ZodeTheme::light(),
        );

        let text = painter.texts.join("\n");
        assert!(text.contains("搜索即将支持"));
        assert!(!text.contains("搜索能力"));
        assert!(!text.contains("搜索技能"));
        assert_eq!(painter.svg_strokes, 0);
    }
}

#[test]
fn narrow_skills_empty_copy_is_measured_centered_and_kept_inside_its_card() {
    let mut state = demo_state();
    state.presentation.route = ShellRoute::Integrations(IntegrationsTab::Skills);
    let surface = Rect::xywh(0.0, 0.0, 320.0, 720.0);
    let empty = Rect::xywh(0.0, 252.0, 320.0, 136.0);
    let mut painter = TextCapture::default();

    IntegrationsPage::paint(&mut painter, surface, &state, &ZodeTheme::light());

    for (content, font_size, weight) in [
        ("尚未接入技能目录", 15.0, 600),
        ("接入真实目录后，这里将展示可用技能。", 13.0, 400),
    ] {
        assert!(painter
            .measurements
            .contains(&(content.to_string(), font_size, weight)));
        let draw = painter
            .text_draws
            .iter()
            .find(|draw| draw.content == content)
            .expect("empty-state copy is painted");
        assert_eq!((draw.font_size, draw.weight), (font_size, weight));
        let width = estimated_text_width(content, font_size);
        let expected_x = empty.origin.x + (empty.size.x - width) / 2.0;
        assert!((draw.origin.x - expected_x).abs() <= 0.01);
        assert!(draw.origin.x >= empty.origin.x);
        assert!(draw.origin.x + width <= empty.origin.x + empty.size.x);
    }
}
