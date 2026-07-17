use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    AttachmentMetadata, ComposerState, GoalProgress, ProjectState, TranscriptItem, TranscriptState,
};
use zode_app_ui::{
    Composer, ComposerController, ImeEvent, Insets, RectExt, WorkspaceShell, WorkspaceSnapshot,
    ZodeTheme, COMPOSER_ID,
};
use zode_node_protocol::{SessionLocator, WorkspaceUri};

#[derive(Default)]
struct TextCapture {
    texts: Vec<String>,
}

impl Painter for TextCapture {
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
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

#[test]
fn composer_uses_text_area_multiline_layout() {
    let state = ComposerState {
        draft: "first line\nsecond line".into(),
        focused: true,
        ..ComposerState::default()
    };
    let mut painter = TextCapture::default();

    Composer::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 500.0, 120.0),
        &state,
        &ZodeTheme::light(),
    );

    assert!(painter.texts.iter().any(|text| text == "first line"));
    assert!(painter.texts.iter().any(|text| text == "second line"));
    assert!(!painter
        .texts
        .iter()
        .any(|text| text == "first line\nsecond line"));
}

#[test]
fn composer_paints_live_ime_preedit() {
    let state = ComposerState {
        focused: true,
        ..ComposerState::default()
    };
    let mut controller = ComposerController::fixture("prefix ");
    controller.ime(ImeEvent::Update {
        text: "中文".into(),
        cursor: Some("中文".len()),
    });
    let mut painter = TextCapture::default();

    Composer::paint_input(
        &mut painter,
        Rect::xywh(0.0, 0.0, 500.0, 120.0),
        controller.input_state(),
        &state,
        &ZodeTheme::light(),
    );

    assert!(painter.texts.iter().any(|text| text.contains("中文")));
}

#[test]
fn composer_does_not_fabricate_a_git_branch_without_environment_data() {
    let state = ComposerState::default();
    let mut painter = TextCapture::default();

    Composer::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 500.0, 120.0),
        &state,
        &ZodeTheme::light(),
    );

    assert!(!painter.texts.iter().any(|text| text == "main"));
}

#[test]
fn composer_paints_only_the_verified_environment_branch() {
    let state = ComposerState::default();
    let mut painter = TextCapture::default();

    Composer::paint_with_branch(
        &mut painter,
        Rect::xywh(0.0, 0.0, 500.0, 120.0),
        &state,
        Some("codex/zode-jian-desktop"),
        &ZodeTheme::light(),
    );

    assert!(painter
        .texts
        .iter()
        .any(|text| text == "codex/zode-jian-desktop"));
    assert!(!painter.texts.iter().any(|text| text == "main"));
}

#[test]
fn composer_hides_unknown_permission_and_connection_modes() {
    let state = ComposerState::default();
    let mut painter = TextCapture::default();

    Composer::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 500.0, 120.0),
        &state,
        &ZodeTheme::light(),
    );

    assert!(!painter.texts.iter().any(|text| text == "完全访问"));
    assert!(!painter.texts.iter().any(|text| text == "本地"));
}

#[test]
fn composer_paints_only_explicit_runtime_context_and_permission() {
    let state = ComposerState {
        sandbox_label: "工作区写入".into(),
        ..ComposerState::default()
    };
    let mut painter = TextCapture::default();

    Composer::paint_with_context(
        &mut painter,
        Rect::xywh(0.0, 0.0, 500.0, 120.0),
        &state,
        Some("本地"),
        Some("codex/desktop"),
        &ZodeTheme::light(),
    );

    for expected in ["本地", "codex/desktop", "工作区写入"] {
        assert!(painter.texts.iter().any(|text| text == expected));
    }
}

#[test]
fn composer_attachment_strip_appears_only_with_metadata() {
    let rect = Rect::xywh(0.0, 0.0, 500.0, 196.0);
    let without = Composer::layout(rect, &ComposerState::default());
    assert!(without.attachments.is_none());
    assert!((without.context.size.y - 44.0).abs() <= 4.0);
    assert!((without.input.size.y - 100.0).abs() <= 2.0);

    let state = ComposerState {
        attachments: vec![AttachmentMetadata {
            id: "attachment-1".into(),
            path: None,
            display_name: "reference.png".into(),
            media_type: "image/png".into(),
            width: Some(640),
            height: Some(360),
            byte_len: 2_048,
        }],
        ..ComposerState::default()
    };
    let with = Composer::layout(rect, &state);
    assert!(with.attachments.is_some());
    assert!((with.context.size.y - 44.0).abs() <= 4.0);
    assert!((with.input.size.y - 100.0).abs() <= 2.0);
}

#[test]
fn workspace_snapshot_drives_the_full_composer_stack_geometry() {
    let empty = ComposerState::default();
    let mut state = zode_app_model::demo_state();
    state.composer = empty;
    let without = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    assert!((without.layout.composer.height() - 144.0).abs() <= f32::EPSILON);
    let without_input = without.node(COMPOSER_ID).expect("composer input node").rect;
    assert!((without_input.height() - 100.0).abs() <= f32::EPSILON);
    assert!((without_input.min_y() - without.layout.composer.min_y() - 44.0).abs() <= f32::EPSILON);

    state.composer.attachments.push(AttachmentMetadata {
        id: "attachment-1".into(),
        path: None,
        display_name: "reference.png".into(),
        media_type: "image/png".into(),
        width: Some(640),
        height: Some(360),
        byte_len: 2_048,
    });
    let with = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    assert!((with.layout.composer.height() - 196.0).abs() <= f32::EPSILON);
    let with_input = with.node(COMPOSER_ID).expect("composer input node").rect;
    assert!((with_input.height() - 100.0).abs() <= f32::EPSILON);
    assert!((with_input.min_y() - with.layout.composer.min_y() - 96.0).abs() <= f32::EPSILON);
}

#[test]
fn workspace_shell_paints_the_full_stack_instead_of_only_the_input_node() {
    let mut state = zode_app_model::demo_state();
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    state.projects.push(ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
    state.active_workspace = Some(workspace);
    state.composer.attachments.push(AttachmentMetadata {
        id: "attachment-1".into(),
        path: None,
        display_name: "reference.png".into(),
        media_type: "image/png".into(),
        width: Some(640),
        height: Some(360),
        byte_len: 2_048,
    });
    let mut painter = TextCapture::default();

    WorkspaceShell::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 1_440.0, 1_080.0),
        Insets::ZERO,
        &state,
        &ZodeTheme::light(),
    );

    assert!(painter.texts.iter().any(|text| text == "zode"));
    assert!(painter
        .texts
        .iter()
        .any(|text| text.contains("reference.png")));
}

#[test]
fn workspace_shell_omits_unknown_workspace_and_projects_the_real_available_label() {
    let mut state = zode_app_model::demo_state();
    let mut painter = TextCapture::default();
    WorkspaceShell::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 1_440.0, 1_080.0),
        Insets::ZERO,
        &state,
        &ZodeTheme::light(),
    );
    assert!(!painter.texts.iter().any(|text| text == "zode"));
    assert!(painter.texts.iter().any(|text| text == "本地"));

    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    state.projects.push(ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
    state.active_workspace = Some(workspace);
    let mut painter = TextCapture::default();
    WorkspaceShell::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 1_440.0, 1_080.0),
        Insets::ZERO,
        &state,
        &ZodeTheme::light(),
    );
    assert!(painter.texts.iter().any(|text| text == "zode"));
}

#[test]
fn busy_composer_context_uses_only_explicit_goal_progress() {
    let mut state = zode_app_model::demo_state();
    let session = SessionLocator::new(state.host.node_id, "goal-context");
    state.current_session = Some(session.clone());
    state.transcripts.insert(
        session.clone(),
        TranscriptState {
            items: vec![TranscriptItem::GoalProgress(GoalProgress {
                id: "goal-1".into(),
                title: "Visual rebuild".into(),
                completed: 3,
                total: 7,
            })],
            busy: true,
            follow_tail: false,
            ..TranscriptState::default()
        },
    );
    let mut painter = TextCapture::default();
    WorkspaceShell::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 1_440.0, 1_080.0),
        Insets::ZERO,
        &state,
        &ZodeTheme::light(),
    );
    assert!(painter
        .texts
        .iter()
        .any(|text| text == "Visual rebuild · 3 / 7"));

    state.transcripts.get_mut(&session).unwrap().items = vec![TranscriptItem::Status {
        code: "running".into(),
        message: "Indexing workspace".into(),
    }];
    let mut painter = TextCapture::default();
    WorkspaceShell::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 1_440.0, 1_080.0),
        Insets::ZERO,
        &state,
        &ZodeTheme::light(),
    );
    assert!(!painter
        .texts
        .iter()
        .any(|text| text.contains("Indexing workspace ·")));
}

#[test]
fn composer_attachment_metadata_has_stable_accessibility_nodes() {
    let mut state = zode_app_model::demo_state();
    for (id, width) in [("attachment-1", 640), ("attachment-2", 1280)] {
        state.composer.attachments.push(AttachmentMetadata {
            id: id.into(),
            path: None,
            display_name: "same.png".into(),
            media_type: "image/png".into(),
            width: Some(width),
            height: Some(360),
            byte_len: 2_048,
        });
    }

    let snapshot = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    let attachments = snapshot
        .nodes
        .iter()
        .filter(|node| node.name == "附件 same.png")
        .collect::<Vec<_>>();

    assert_eq!(attachments.len(), 2);
    assert_ne!(attachments[0].id, attachments[1].id);
    assert!(attachments.iter().all(|node| node
        .value
        .as_deref()
        .is_some_and(|value| value.contains("image/png"))));
    assert!(attachments[0].actions.is_empty());
    assert!(attachments[1].actions.is_empty());
}
