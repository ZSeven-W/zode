use jian_core::text_input::TextInputState;
use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_ui::{
    Composer, SemanticIcon, WidgetId, ZodeTheme, COMPOSER_PROJECT_ID, PROJECT_DETACH_ID,
};

#[derive(Default)]
struct PaintCapture {
    texts: Vec<String>,
    round_fills: Vec<(Rect, f32, Color)>,
    round_strokes: Vec<Rect>,
    svg_paths: Vec<String>,
}

impl Painter for PaintCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, _origin: Point2D) {
        self.texts.push(
            layout
                .runs()
                .iter()
                .map(|run| run.content.as_str())
                .collect(),
        );
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        self.round_fills.push((rect, radius, color));
    }
    fn stroke_round_rect(&mut self, rect: Rect, _radius: f32, _color: Color, _width: f32) {
        self.round_strokes.push(rect);
    }
    fn stroke_svg_path(
        &mut self,
        d: &str,
        _top_left: Point2D,
        _size: f32,
        _color: Color,
        _width: f32,
    ) {
        self.svg_paths.push(d.to_owned());
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

fn layout(width: f32) -> zode_app_ui::ComposerContextLayout {
    Composer::context_interaction_layout(
        Rect::xywh(0.0, 0.0, width, 138.0),
        &zode_app_model::demo_state(),
        Some("zode"),
        Some("本地"),
        Some("main"),
    )
}

#[test]
fn new_task_context_has_distinct_chip_and_detach_targets() {
    let layout = layout(736.0);
    let project = layout.project.expect("project chip");
    let location = layout.location.expect("location chip");
    let branch = layout.branch.expect("branch chip");
    let detach = layout.detach.expect("project detach target");

    assert_eq!(project.rect.size.y, 28.0);
    assert_eq!(location.rect.size.y, 28.0);
    assert_eq!(branch.rect.size.y, 28.0);
    assert!(project.action_rect.origin.x >= detach.origin.x + detach.size.x);
    assert_eq!(layout.hit_test(center(detach)), Some(PROJECT_DETACH_ID));
    assert_eq!(
        layout.hit_test(center(project.action_rect)),
        Some(project.id)
    );

    let ids = [project.id, location.id, branch.id, PROJECT_DETACH_ID];
    for (index, id) in ids.iter().copied().enumerate() {
        assert!(!ids[..index].contains(&id), "context targets stay unique");
    }
}

#[test]
fn projectless_new_task_exposes_a_select_project_chip_without_detach() {
    let mut state = zode_app_model::demo_state();
    state.current_session = None;
    state.active_workspace = None;
    let rect = Rect::xywh(0.0, 0.0, 736.0, 138.0);
    let layout = Composer::context_interaction_layout(rect, &state, None, Some("本地"), None);

    let project = layout.project.expect("select-project chip");
    assert_eq!(project.id, COMPOSER_PROJECT_ID);
    assert_eq!(
        layout.hit_test(center(project.action_rect)),
        Some(COMPOSER_PROJECT_ID)
    );
    assert!(layout.detach.is_none());

    let mut painter = PaintCapture::default();
    Composer::paint_input_with_workspace_app_context_interactions(
        &mut painter,
        rect,
        &TextInputState::default(),
        &state,
        None,
        Some("本地"),
        None,
        None,
        None,
        None,
        None,
        &ZodeTheme::light(),
    );
    assert!(
        painter.texts.iter().any(|text| text == "选择项目"),
        "painted texts: {:?}",
        painter.texts
    );

    let mut tooltip_painter = PaintCapture::default();
    Composer::paint_input_with_workspace_app_context_interactions(
        &mut tooltip_painter,
        rect,
        &TextInputState::default(),
        &state,
        None,
        Some("本地"),
        None,
        None,
        None,
        Some(COMPOSER_PROJECT_ID),
        None,
        &ZodeTheme::light(),
    );
    assert!(tooltip_painter.texts.iter().any(|text| text == "选择项目"));
    assert!(!tooltip_painter
        .texts
        .iter()
        .any(|text| text == "更改此任务的项目"));
}

#[test]
fn narrow_context_clips_the_last_visible_chip_and_omits_later_chips() {
    let layout = layout(180.0);
    let project = layout.project.expect("project keeps first priority");
    let location = layout.location.expect("location uses the remaining width");

    assert!(project.rect.origin.x + project.rect.size.x <= location.rect.origin.x);
    assert!(location.label_rect.size.x < 28.0);
    assert!(layout.branch.is_none());
}

#[test]
fn existing_task_context_does_not_expose_migration_chips() {
    let mut state = zode_app_model::demo_state();
    state.current_session = Some(zode_node_protocol::SessionLocator::new(
        state.host.node_id,
        "existing",
    ));
    let layout = Composer::context_interaction_layout(
        Rect::xywh(0.0, 0.0, 736.0, 138.0),
        &state,
        Some("zode"),
        Some("本地"),
        Some("main"),
    );

    assert!(layout.project.is_none());
    assert!(layout.location.is_none());
    assert!(layout.branch.is_none());
    assert!(layout.detach.is_none());
}

#[test]
fn chips_use_borderless_pill_states_and_product_tooltips() {
    let state = zode_app_model::demo_state();
    let rect = Rect::xywh(0.0, 0.0, 736.0, 138.0);
    let layout = layout(736.0);
    let theme = ZodeTheme::light();
    let interactions: [(WidgetId, &str); 3] = [
        (layout.project.unwrap().id, "更改此任务的项目"),
        (layout.location.unwrap().id, "选择任务的运行位置"),
        (layout.branch.unwrap().id, "切换任务分支"),
    ];

    for (id, tooltip) in interactions {
        let mut painter = PaintCapture::default();
        Composer::paint_input_with_workspace_app_context_interactions(
            &mut painter,
            rect,
            &TextInputState::default(),
            &state,
            Some("zode"),
            Some("本地"),
            Some("main"),
            None,
            None,
            Some(id),
            Some(id),
            &theme,
        );
        let chip = [layout.project, layout.location, layout.branch]
            .into_iter()
            .flatten()
            .find(|chip| chip.id == id)
            .unwrap();
        assert!(painter
            .round_fills
            .iter()
            .any(|(rect, radius, color)| *rect == chip.rect
                && *radius == 14.0
                && *color == theme.tokens.accent));
        assert!(!painter.round_strokes.contains(&chip.rect));
        assert!(painter.texts.iter().any(|text| text == tooltip));
    }
}

#[test]
fn project_chip_swaps_folder_for_close_only_while_hovered() {
    let state = zode_app_model::demo_state();
    let rect = Rect::xywh(0.0, 0.0, 736.0, 138.0);
    let project = layout(736.0).project.expect("project chip");
    let theme = ZodeTheme::light();

    let mut idle = PaintCapture::default();
    Composer::paint_input_with_workspace_app_context_interactions(
        &mut idle,
        rect,
        &TextInputState::default(),
        &state,
        Some("zode"),
        Some("本地"),
        Some("main"),
        None,
        None,
        None,
        None,
        &theme,
    );
    assert!(idle
        .svg_paths
        .iter()
        .any(|path| path == SemanticIcon::Folder.path()));
    assert!(!idle
        .svg_paths
        .iter()
        .any(|path| path == SemanticIcon::Close.path()));
    assert!(!idle
        .round_fills
        .iter()
        .any(|(rect, _, _)| *rect == project.rect));

    let mut hovered = PaintCapture::default();
    Composer::paint_input_with_workspace_app_context_interactions(
        &mut hovered,
        rect,
        &TextInputState::default(),
        &state,
        Some("zode"),
        Some("本地"),
        Some("main"),
        None,
        None,
        Some(COMPOSER_PROJECT_ID),
        None,
        &theme,
    );
    assert!(hovered
        .svg_paths
        .iter()
        .any(|path| path == SemanticIcon::Close.path()));
    assert!(!hovered
        .svg_paths
        .iter()
        .any(|path| path == SemanticIcon::Folder.path()));
    assert!(hovered
        .round_fills
        .iter()
        .any(|(rect, radius, color)| *rect == project.rect
            && *radius == 14.0
            && *color == theme.tokens.accent));

    let mut detach_hovered = PaintCapture::default();
    Composer::paint_input_with_workspace_app_context_interactions(
        &mut detach_hovered,
        rect,
        &TextInputState::default(),
        &state,
        Some("zode"),
        Some("本地"),
        Some("main"),
        None,
        None,
        Some(PROJECT_DETACH_ID),
        None,
        &theme,
    );
    assert!(detach_hovered
        .svg_paths
        .iter()
        .any(|path| path == SemanticIcon::Close.path()));
    assert!(detach_hovered
        .round_fills
        .iter()
        .any(|(rect, _, color)| *rect == project.rect && *color == theme.tokens.accent));
}
