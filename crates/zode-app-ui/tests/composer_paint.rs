use accesskit::Action;
use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    AttachmentMetadata, ComposerState, EnvironmentSnapshot, GoalProgress, LoadState, ProjectState,
    SessionPresentationState, TranscriptItem, TranscriptState,
};
use zode_app_ui::{
    Composer, ComposerController, ImeEvent, Insets, Key, RectExt, WorkspaceShell,
    WorkspaceSnapshot, ZodeTheme, COMPOSER_ID, PROJECT_DETACH_ID, SEND_ID,
};
use zode_node_protocol::{
    ApprovalMode, SandboxMode, SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri,
};

#[derive(Default)]
struct TextCapture {
    texts: Vec<String>,
    text_lines: Vec<(String, Point2D, f32)>,
    text_colors: Vec<(String, Color)>,
    icons: Vec<(&'static str, Point2D, f32)>,
    icon_colors: Vec<(&'static str, Color)>,
    fill_rects: Vec<Rect>,
    rect_fills: Vec<(Rect, Color)>,
    round_fills: Vec<(Rect, Color)>,
    round_fill_details: Vec<(Rect, f32, Color)>,
}

impl Painter for TextCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.fill_rects.push(rect);
        self.rect_fills.push((rect, color));
    }
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, origin: Point2D) {
        let text: String = layout
            .runs()
            .iter()
            .map(|run| run.content.as_str())
            .collect();
        if let Some(run) = layout.runs().first() {
            self.text_lines.push((text.clone(), origin, run.font_size));
            self.text_colors.push((
                text.clone(),
                Color::rgba_u8(
                    run.color.r(),
                    run.color.g(),
                    run.color.b(),
                    f32::from(run.color.a()) / 255.0,
                ),
            ));
        }
        self.texts.push(text);
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        self.round_fills.push((rect, color));
        self.round_fill_details.push((rect, radius, color));
    }
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
    fn stroke_svg_path(
        &mut self,
        d: &str,
        top_left: Point2D,
        size: f32,
        color: Color,
        _width: f32,
    ) {
        let semantic = zode_app_ui::SemanticIcon::ALL
            .into_iter()
            .find(|icon| icon.path() == d)
            .expect("composer uses a registered semantic icon");
        self.icons.push((semantic.path(), top_left, size));
        self.icon_colors.push((semantic.path(), color));
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
fn empty_new_task_uses_the_reference_placeholder() {
    let state = ComposerState::default();
    let mut painter = TextCapture::default();

    Composer::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 736.0, 100.0),
        &state,
        &ZodeTheme::light(),
    );

    assert!(painter.texts.iter().any(|text| text == "随心输入"));
    assert!(!painter
        .texts
        .iter()
        .any(|text| text == "向 Zode 描述一个任务"));
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
fn composer_ime_cursor_area_tracks_the_painted_preedit_caret() {
    let mut state = zode_app_model::demo_state();
    state.composer.focused = true;
    let mut controller = ComposerController::fixture("prefix ");
    controller.ime(ImeEvent::Update {
        text: "中文".into(),
        cursor: Some("中".len()),
    });
    let rect = Rect::xywh(40.0, 500.0, 500.0, 120.0);
    let theme = ZodeTheme::light();
    let mut painted = TextCapture::default();

    Composer::paint_input(
        &mut painted,
        rect,
        controller.input_state(),
        &state.composer,
        &theme,
    );
    let painted_caret = *painted
        .fill_rects
        .last()
        .expect("focused composer paints a caret");
    let mut metrics = TextCapture::default();
    let ime_area =
        Composer::ime_cursor_area(&mut metrics, rect, controller.input_state(), &state, &theme)
            .expect("focused composer exposes its caret area");

    assert_eq!(ime_area, painted_caret);
    assert!(ime_area.origin.y > rect.origin.y);
}

#[test]
fn composer_ime_cursor_area_remains_available_for_a_selection() {
    let mut state = zode_app_model::demo_state();
    state.composer.focused = true;
    let mut controller = ComposerController::fixture("selected text");
    let _ = controller.key(Key::Character("a".into()), zode_app_ui::Modifiers::SUPER);
    let rect = Rect::xywh(40.0, 500.0, 500.0, 120.0);

    let area = Composer::ime_cursor_area(
        &mut TextCapture::default(),
        rect,
        controller.input_state(),
        &state,
        &ZodeTheme::light(),
    )
    .expect("selection focus still provides an IME insertion anchor");

    assert!(Composer::layout_for_state(rect, &state)
        .input
        .contains(area.origin));
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
    assert!(painter
        .icons
        .iter()
        .any(|(path, _, _)| *path == zode_app_ui::SemanticIcon::Host.path()));
}

#[test]
fn composer_permission_trigger_matches_each_approval_level() {
    let theme = ZodeTheme::light();
    let cases = [
        (
            ApprovalMode::Request,
            SandboxMode::WorkspaceWrite,
            false,
            "请求批准",
            zode_app_ui::SemanticIcon::Host,
            theme.tokens.muted_foreground,
        ),
        (
            ApprovalMode::Auto,
            SandboxMode::WorkspaceWrite,
            true,
            "替我审批",
            zode_app_ui::SemanticIcon::Refresh,
            theme.tokens.muted_foreground,
        ),
        (
            ApprovalMode::Full,
            SandboxMode::Off,
            false,
            "完全访问",
            zode_app_ui::SemanticIcon::ShieldAlert,
            theme.composer_permission,
        ),
        (
            ApprovalMode::Request,
            SandboxMode::ReadOnly,
            false,
            "自定义",
            zode_app_ui::SemanticIcon::Settings,
            theme.tokens.muted_foreground,
        ),
    ];

    for (approval, sandbox, network, label, expected_icon, expected_color) in cases {
        let state = ComposerState {
            sandbox_label: label.into(),
            approval_mode: approval,
            sandbox_mode: sandbox,
            sandbox_network: network,
            ..ComposerState::default()
        };
        let mut painter = TextCapture::default();

        Composer::paint(
            &mut painter,
            Rect::xywh(0.0, 0.0, 500.0, 120.0),
            &state,
            &theme,
        );

        assert!(
            painter
                .icon_colors
                .iter()
                .any(|(path, color)| *path == expected_icon.path() && *color == expected_color),
            "wrong icon or icon color for {label}"
        );
        assert!(
            painter
                .text_colors
                .iter()
                .any(|(text, color)| text == label && *color == expected_color),
            "wrong text color for {label}"
        );
    }
}

#[test]
fn composer_context_uses_semantic_icons_centered_in_the_context_row() {
    let state = ComposerState::default();
    let mut painter = TextCapture::default();
    let rect = Rect::xywh(0.0, 0.0, 500.0, 144.0);
    let context = Composer::layout(rect, &state).context;

    Composer::paint_input_with_workspace_context(
        &mut painter,
        rect,
        &jian_core::text_input::TextInputState::default(),
        &state,
        Some("zode"),
        Some("本地"),
        Some("main"),
        None,
        &ZodeTheme::light(),
    );

    for expected in [
        zode_app_ui::SemanticIcon::Folder,
        zode_app_ui::SemanticIcon::Host,
        zode_app_ui::SemanticIcon::Branch,
    ] {
        let (_, origin, size) = painter
            .icons
            .iter()
            .find(|(path, _, _)| *path == expected.path())
            .expect("context semantic icon is painted");
        assert_eq!(*size, 14.0);
        assert_eq!(origin.y, context.min_y() + (context.height() - size) / 2.0);
    }
    for expected in ["zode", "本地", "main"] {
        assert!(painter.texts.iter().any(|text| text == expected));
        let (_, origin, size) = painter
            .text_lines
            .iter()
            .find(|(text, _, _)| text == expected)
            .expect("context label uses the shared TextBox path");
        assert!((origin.y + size / 2.0 - (context.min_y() + context.height() / 2.0)).abs() <= 0.01);
    }
}

#[test]
fn composer_attachment_strip_appears_only_with_metadata() {
    let rect = Rect::xywh(0.0, 0.0, 500.0, 190.0);
    let without = Composer::layout(rect, &ComposerState::default());
    assert!(without.attachments.is_none());
    assert!((without.context.size.y - 38.0).abs() <= f32::EPSILON);
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
    let attachment = with.attachments.expect("attachment strip");
    assert!((attachment.height() - 52.0).abs() <= f32::EPSILON);
    assert!((with.context.size.y - 38.0).abs() <= f32::EPSILON);
    assert!((with.input.size.y - 100.0).abs() <= 2.0);

    let mut shell_state = zode_app_model::demo_state();
    shell_state.composer = state;
    let snapshot = WorkspaceSnapshot::build(&shell_state, 1_440.0, 1_080.0, Insets::ZERO);
    let attachment = snapshot
        .nodes
        .iter()
        .find(|node| node.name == "附件 reference.png")
        .expect("attachment metadata node");
    assert_eq!(attachment.rect.size.x, 240.0);
}

#[test]
fn workspace_snapshot_drives_the_full_composer_stack_geometry() {
    let empty = ComposerState::default();
    let mut state = zode_app_model::demo_state();
    state.composer = empty;
    let without = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    assert!((without.layout.composer.height() - 138.0).abs() <= f32::EPSILON);
    let without_layout = Composer::layout_for_state(without.layout.composer, &state);
    assert!((without_layout.context.height() - 38.0).abs() <= f32::EPSILON);
    assert!(
        (without_layout.context.min_x() - without_layout.input.min_x() - 14.0).abs()
            <= f32::EPSILON
    );
    assert!(
        (without_layout.input.max_x() - without_layout.context.max_x() - 14.0).abs()
            <= f32::EPSILON
    );
    assert!((without_layout.context.max_y() - without_layout.input.min_y()).abs() <= f32::EPSILON);
    let without_input = without.node(COMPOSER_ID).expect("composer input node").rect;
    assert_eq!(without_input, without_layout.input);
    assert!((without_input.height() - 100.0).abs() <= f32::EPSILON);
    let overlap_point = Point2D::new(without_input.min_x() + 100.0, without_input.min_y() + 1.0);
    assert_eq!(without.hit_test(overlap_point), Some(COMPOSER_ID));

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
    assert!(
        (with.layout.composer.height() - without.layout.composer.height() - 52.0).abs()
            <= f32::EPSILON
    );
    let with_layout = Composer::layout_for_state(with.layout.composer, &state);
    assert!((with_layout.context.height() - 38.0).abs() <= f32::EPSILON);
    assert!(
        (with_layout.attachments.expect("attachment strip").height() - 52.0).abs() <= f32::EPSILON
    );
    let with_input = with.node(COMPOSER_ID).expect("composer input node").rect;
    assert_eq!(with_input, with_layout.input);
    assert!((with_input.height() - 100.0).abs() <= f32::EPSILON);
}

/// The context rail (project/location/branch migration chips) only makes
/// sense before a conversation exists. Once the active session's transcript
/// holds a message, the rail must vanish entirely and its 38px collapses
/// back into the transcript, not just stop being interactive.
#[test]
fn context_bar_collapses_once_the_active_transcript_has_a_message() {
    let new_task = zode_app_model::demo_state();
    let new_task_snapshot = WorkspaceSnapshot::build(&new_task, 1_440.0, 1_080.0, Insets::ZERO);

    let mut started = zode_app_model::demo_state();
    let session = SessionLocator::new(started.host.node_id, "just-started");
    started.current_session = Some(session.clone());
    started.transcripts.insert(
        session,
        TranscriptState {
            items: vec![TranscriptItem::user_text("build the renderer")],
            ..TranscriptState::default()
        },
    );
    let started_snapshot = WorkspaceSnapshot::build(&started, 1_440.0, 1_080.0, Insets::ZERO);

    let bar_height = 38.0;
    assert!(
        (new_task_snapshot.layout.composer.height()
            - started_snapshot.layout.composer.height()
            - bar_height)
            .abs()
            <= f32::EPSILON,
        "hiding the bar must shrink the reserved composer height by exactly its own height"
    );
    assert!(
        (started_snapshot.layout.composer.origin.y
            - new_task_snapshot.layout.composer.origin.y
            - bar_height)
            .abs()
            <= f32::EPSILON,
        "the composer, being bottom anchored, must move down by the reclaimed height"
    );
    assert!(
        (started_snapshot.layout.transcript.height()
            - new_task_snapshot.layout.transcript.height()
            - bar_height)
            .abs()
            <= f32::EPSILON,
        "the transcript must grow into exactly the space the bar gave up"
    );

    // With a zero-height context rect, `context::paint_interactive` early-returns
    // (it bails on `rect.size.y <= 0.0` before painting the background pill or
    // any chip), so proving the layout collapsed is enough to prove nothing
    // paints there; a whole-shell text search would be unreliable here since
    // both the workspace label and the connection label ("本地") legitimately
    // recur elsewhere in the shell (sidebar project list, account name, etc).
    let started_layout = Composer::layout_for_state(started_snapshot.layout.composer, &started);
    assert_eq!(started_layout.context.height(), 0.0);
    let context = Composer::context_interaction_layout(
        started_snapshot.layout.composer,
        &started,
        Some("zode"),
        Some("本地"),
        Some("main"),
    );
    assert!(context.project.is_none());
    assert!(context.location.is_none());
    assert!(context.branch.is_none());
}

/// A restored historical session (transcript already populated before the
/// composer ever paints) must start with the bar already collapsed, same as
/// the moment right after sending the first message.
#[test]
fn restored_session_with_history_never_shows_the_context_bar() {
    let mut state = zode_app_model::demo_state();
    let session = SessionLocator::new(state.host.node_id, "restored");
    state.current_session = Some(session.clone());
    state.transcripts.insert(
        session,
        TranscriptState {
            items: vec![
                TranscriptItem::user_text("earlier question"),
                TranscriptItem::assistant_text("earlier answer"),
            ],
            ..TranscriptState::default()
        },
    );
    let snapshot = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    let layout = Composer::layout_for_state(snapshot.layout.composer, &state);
    assert_eq!(layout.context.height(), 0.0);
}

/// A live goal-progress pill has no analog in the environment panel, so it
/// keeps reserving the rail's height even once the conversation is busy.
#[test]
fn goal_progress_keeps_the_context_rail_reserved_mid_conversation() {
    let new_task = zode_app_model::demo_state();
    let new_task_snapshot = WorkspaceSnapshot::build(&new_task, 1_440.0, 1_080.0, Insets::ZERO);

    let mut with_goal = zode_app_model::demo_state();
    let session = SessionLocator::new(with_goal.host.node_id, "goal-context");
    with_goal.current_session = Some(session.clone());
    with_goal.transcripts.insert(
        session,
        TranscriptState {
            items: vec![TranscriptItem::GoalProgress(GoalProgress {
                id: "goal-1".into(),
                title: "Visual rebuild".into(),
                completed: 3,
                total: 7,
            })],
            busy: true,
            ..TranscriptState::default()
        },
    );
    let with_goal_snapshot = WorkspaceSnapshot::build(&with_goal, 1_440.0, 1_080.0, Insets::ZERO);

    assert_eq!(
        with_goal_snapshot.layout.composer.height(),
        new_task_snapshot.layout.composer.height()
    );
    let layout = Composer::layout_for_state(with_goal_snapshot.layout.composer, &with_goal);
    assert!((layout.context.height() - 38.0).abs() <= f32::EPSILON);
}

#[test]
fn context_rail_paints_behind_the_later_input_card() {
    let state = ComposerState::default();
    let rect = Rect::xywh(0.0, 0.0, 736.0, 138.0);
    let layout = Composer::layout(rect, &state);
    let theme = ZodeTheme::light();
    let mut painter = TextCapture::default();

    Composer::paint_input_with_workspace_context(
        &mut painter,
        rect,
        &jian_core::text_input::TextInputState::default(),
        &state,
        Some("zode"),
        Some("本地"),
        Some("main"),
        None,
        &theme,
    );

    let (rail_index, rail) = painter
        .round_fills
        .iter()
        .enumerate()
        .find(|(_, (_, color))| *color == theme.tokens.muted)
        .map(|(index, (rect, _))| (index, *rect))
        .expect("muted context rail surface");
    let (_, rail_radius, _) = painter
        .round_fill_details
        .iter()
        .find(|(rect, _, color)| *rect == rail && *color == theme.tokens.muted)
        .expect("context rail retains its rounded top surface");
    let (square_tail, _) = painter
        .rect_fills
        .iter()
        .find(|(rect, color)| {
            *color == theme.tokens.muted
                && rect.min_x() == rail.min_x()
                && rect.max_x() == rail.max_x()
                && rect.max_y() == rail.max_y()
        })
        .expect("context rail fills its square lower corners to both edges");
    let (input_index, _) = painter
        .round_fills
        .iter()
        .enumerate()
        .find(|(_, (rect, color))| *rect == layout.input && *color == theme.tokens.card)
        .expect("input card surface");

    assert_eq!(rail.min_x(), layout.context.min_x());
    assert_eq!(rail.max_x(), layout.context.max_x());
    assert_eq!(*rail_radius, 18.0);
    assert!((rail.height() - 44.0).abs() <= f32::EPSILON);
    assert!((rail.max_y() - layout.input.min_y() - 6.0).abs() <= f32::EPSILON);
    assert!(square_tail.min_y() > rail.min_y());
    assert!(square_tail.min_y() < layout.context.max_y());
    assert_eq!(layout.context, Rect::xywh(14.0, 0.0, 708.0, 38.0));
    assert_eq!(layout.input, Rect::xywh(0.0, 38.0, 736.0, 100.0));
    assert!(
        rail_index < input_index,
        "input card must cover the rail tail"
    );
}

#[test]
fn shallow_context_rail_keeps_a_rounded_top_cap() {
    let state = ComposerState::default();
    let rect = Rect::xywh(0.0, 0.0, 736.0, 110.0);
    let theme = ZodeTheme::light();
    let mut painter = TextCapture::default();
    Composer::paint_input(
        &mut painter,
        rect,
        &jian_core::text_input::TextInputState::default(),
        &state,
        &theme,
    );
    let (rail, _) = painter
        .round_fills
        .iter()
        .find(|(_, color)| *color == theme.tokens.muted)
        .expect("shallow rail surface");
    let (tail, _) = painter
        .rect_fills
        .iter()
        .find(|(_, color)| *color == theme.tokens.muted)
        .expect("shallow rail square tail");
    assert!(tail.min_y() > rail.min_y());
}

#[test]
fn empty_draft_disables_send_until_there_is_submittable_content() {
    let mut state = zode_app_model::demo_state();
    state.current_session = None;
    state.composer.draft.clear();

    let empty = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    let send = empty
        .node(SEND_ID)
        .expect("disabled send remains discoverable");
    assert_eq!(send.name, "发送");
    assert!(send.disabled);
    assert!(send.actions.is_empty());
    assert_ne!(
        empty.hit_test(Point2D::new(
            send.rect.min_x() + send.rect.width() / 2.0,
            send.rect.min_y() + send.rect.height() / 2.0,
        )),
        Some(SEND_ID),
    );

    state.composer.draft = "  构建一个任务  ".into();
    let ready = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    let send = ready.node(SEND_ID).expect("enabled send");
    assert!(!send.disabled);
    assert!(send.actions.contains(&Action::Click));
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
fn ordinary_offscreen_shell_entry_paints_the_open_project_picker() {
    let mut state = zode_app_model::demo_state();
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    state.projects.push(ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.active_workspace = Some(workspace);
    state.project_picker.open = true;
    state.project_picker.search = "zod".into();
    let mut painter = TextCapture::default();

    WorkspaceShell::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 1_440.0, 1_080.0),
        Insets::ZERO,
        &state,
        &ZodeTheme::light(),
    );

    for expected in [
        "我们应该在 ",
        " 中构建什么？",
        "zod",
        "zode",
        "新建项目",
        "不在项目中工作",
    ] {
        assert!(
            painter.texts.iter().any(|text| text == expected),
            "open picker paints {expected}"
        );
    }
}

#[test]
fn projectless_session_hides_scratch_workspace_and_branch_from_composer() {
    let mut state = zode_app_model::demo_state();
    let scratch_root = WorkspaceUri::new("file:///private/tmp/zode-projectless").unwrap();
    let scratch_child =
        WorkspaceUri::new("file:///private/tmp/zode-projectless/task-session").unwrap();
    let session = SessionLocator::new(state.host.node_id, "task-session");
    state.projectless_workspace_root = Some(scratch_root);
    state.current_session = Some(session.clone());
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: scratch_child.clone(),
        title: "Task".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    state
        .transcripts
        .insert(session.clone(), TranscriptState::default());
    state.presentation.sessions.insert(
        session,
        SessionPresentationState {
            context: LoadState::Ready(EnvironmentSnapshot {
                workspace_uri: scratch_child,
                branch: Some("main".into()),
                background_processes: Vec::new(),
                sources: Vec::new(),
            }),
            ..SessionPresentationState::default()
        },
    );
    let snapshot = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    let mut painter = TextCapture::default();
    WorkspaceShell::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 1_440.0, 1_080.0),
        Insets::ZERO,
        &state,
        &ZodeTheme::light(),
    );

    let composer_texts = painter
        .text_lines
        .iter()
        .filter(|(_, origin, _)| snapshot.layout.composer.contains(*origin))
        .map(|(text, _, _)| text)
        .collect::<Vec<_>>();
    assert!(composer_texts.iter().any(|text| text.as_str() == "本地"));
    assert!(!composer_texts.iter().any(|text| text.as_str() == "main"));
    assert!(!composer_texts
        .iter()
        .any(|text| text.contains("task-session") || text.contains("zode-projectless")));
}

#[test]
fn projectless_new_task_starts_with_a_select_project_chip_and_no_detach_action() {
    let mut state = zode_app_model::demo_state();
    state.current_session = None;
    state.active_workspace = None;
    state.projects.clear();
    state.projectless_workspace_root =
        Some(WorkspaceUri::new("file:///private/tmp/zode-projectless").unwrap());

    let snapshot = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    assert!(snapshot.node(PROJECT_DETACH_ID).is_none());

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
        .any(|text| text == "我们应该构建什么？"));
    assert!(!painter.texts.iter().any(|text| text == "我们应该在 "));
    let composer_texts = painter
        .text_lines
        .iter()
        .filter(|(_, origin, _)| snapshot.layout.composer.contains(*origin))
        .map(|(text, _, _)| text.as_str())
        .collect::<Vec<_>>();
    assert!(composer_texts.contains(&"选择项目"));
    assert!(composer_texts.contains(&"本地"));
    assert!(!composer_texts
        .iter()
        .any(|text| text.contains("zode-projectless") || *text == "main"));
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
