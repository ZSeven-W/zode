use accesskit::{Action, Role};
use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    demo_state, AppCommand, ComposerState, MessageQueueState, QueuedMessageId, TranscriptState,
};
use zode_app_ui::{
    Composer, ComposerController, ComposerOutcome, Insets, Key, Modifiers, RectExt, SemanticIcon,
    TerminalGrid, WorkspaceShell, WorkspaceSnapshot, ZodeTheme, COMPOSER_INPUT_H,
    COMPOSER_QUEUE_MAX_VISIBLE, COMPOSER_QUEUE_OVERLAP, COMPOSER_QUEUE_ROW_H, SEND_ID,
};
use zode_node_protocol::SessionLocator;

fn queue_state(count: usize) -> (zode_app_model::ZodeAppState, SessionLocator) {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "queue-session");
    state.current_session = Some(session.clone());
    state
        .transcripts
        .insert(session.clone(), TranscriptState::default());
    let mut queue = MessageQueueState::default();
    for index in 0..count {
        queue
            .enqueue(format!("queued message {index}"), Vec::new())
            .unwrap();
    }
    state.message_queues.insert(session.clone(), queue);
    (state, session)
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

#[test]
fn busy_enter_queues_instead_of_steering_and_stop_stays_explicit() {
    let mut composer = ComposerController::fixture("follow up");
    composer.set_busy(true);

    assert!(matches!(
        composer.key(Key::Enter, Modifiers::NONE),
        ComposerOutcome::Queue(_)
    ));
    assert_eq!(composer.stop(), ComposerOutcome::Stop);
}

#[test]
fn four_row_queue_matches_the_reference_stack_geometry() {
    let (state, _) = queue_state(4);
    let rect = Rect::xywh(100.0, 200.0, 736.0, 270.0);
    let layout = Composer::layout_for_state(rect, &state);
    let queue = Composer::queue_layout(rect, &state).expect("queue layout");

    assert_eq!(queue.rows.len(), COMPOSER_QUEUE_MAX_VISIBLE);
    assert_eq!(layout.input.size.y, COMPOSER_INPUT_H);
    assert_eq!(layout.input.max_y(), rect.max_y());
    assert_eq!(queue.surface.origin.x, rect.origin.x + 20.0);
    assert_eq!(queue.surface.size.x, rect.size.x - 40.0);
    assert_eq!(
        queue.surface.max_y(),
        layout.input.origin.y + COMPOSER_QUEUE_OVERLAP
    );
    for pair in queue.rows.windows(2) {
        assert_eq!(
            pair[1].rect.origin.y - pair[0].rect.origin.y,
            COMPOSER_QUEUE_ROW_H
        );
    }
}

#[test]
fn queue_actions_share_layout_hit_rects_and_session_scoped_commands() {
    let (mut state, session) = queue_state(2);
    let rect = Rect::xywh(100.0, 200.0, 736.0, 212.0);
    let queue = Composer::queue_layout(rect, &state).unwrap();
    let first = &queue.rows[0];

    assert_eq!(
        Composer::command_for_widget(&state, first.guide_id),
        Some(AppCommand::GuideQueuedMessage {
            session: session.clone(),
            id: QueuedMessageId::from_u64(1),
        })
    );
    assert_eq!(
        Composer::command_for_widget(&state, first.delete_id),
        Some(AppCommand::RemoveQueuedMessage {
            session: session.clone(),
            id: QueuedMessageId::from_u64(1),
        })
    );
    assert_eq!(
        Composer::command_for_widget(&state, first.more_id),
        Some(AppCommand::ToggleQueuedMessageMenu {
            session: session.clone(),
            id: QueuedMessageId::from_u64(1),
        })
    );

    state.composer.queue_menu = Some(QueuedMessageId::from_u64(1));
    let open = Composer::queue_layout(rect, &state).unwrap();
    let menu = open.menu.expect("open menu");
    assert_eq!(
        Composer::command_for_widget(&state, menu.edit_id),
        Some(AppCommand::BeginEditQueuedMessage {
            session: session.clone(),
            id: QueuedMessageId::from_u64(1),
        })
    );
    assert_eq!(
        Composer::command_for_widget(&state, menu.close_id),
        Some(AppCommand::ClearMessageQueue { session })
    );
}

#[test]
fn queue_snapshot_uses_dynamic_ids_rects_roles_and_menu_hit_precedence() {
    let (mut state, _) = queue_state(2);
    state.composer.queue_menu = Some(QueuedMessageId::from_u64(1));
    let snapshot = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    let layout = Composer::queue_layout(snapshot.layout.composer, &state).unwrap();
    let first = &layout.rows[0];

    for (id, rect, label) in [
        (first.guide_id, first.guide, "引导排队消息"),
        (first.delete_id, first.delete, "删除排队消息"),
        (first.more_id, first.more, "更多排队操作"),
    ] {
        let node = snapshot.node(id).expect("queue action node");
        assert_eq!(node.rect, rect);
        assert_eq!(node.role, Role::Button);
        assert!(node.name.contains(label));
        assert!(node.actions.contains(&Action::Click));
        let hit = snapshot.hit_test(center(rect));
        assert_eq!(
            hit,
            Some(id),
            "{label}; actual={:?}",
            hit.and_then(|hit| snapshot.node(hit).map(|node| node.name.as_str()))
        );
    }

    let menu = layout.menu.unwrap();
    assert_eq!(snapshot.node(menu.edit_id).unwrap().role, Role::MenuItem);
    assert_eq!(snapshot.node(menu.close_id).unwrap().role, Role::MenuItem);
    assert_eq!(snapshot.hit_test(center(menu.edit)), Some(menu.edit_id));
    assert_eq!(snapshot.hit_test(center(menu.close)), Some(menu.close_id));
}

#[test]
fn queue_ids_include_session_identity() {
    let (first_state, _) = queue_state(1);
    let mut second_state = first_state.clone();
    let second = SessionLocator::new(second_state.host.node_id, "other-session");
    second_state.current_session = Some(second.clone());
    second_state
        .transcripts
        .insert(second.clone(), TranscriptState::default());
    let mut queue = MessageQueueState::default();
    queue.enqueue("same local id".into(), Vec::new()).unwrap();
    second_state.message_queues.insert(second, queue);
    let rect = Rect::xywh(0.0, 0.0, 736.0, 183.0);

    let first = Composer::queue_layout(rect, &first_state).unwrap();
    let second = Composer::queue_layout(rect, &second_state).unwrap();
    assert_ne!(first.rows[0].guide_id, second.rows[0].guide_id);
    assert_ne!(first.rows[0].delete_id, second.rows[0].delete_id);
    assert_ne!(first.rows[0].more_id, second.rows[0].more_id);
}

#[derive(Default)]
struct CapturePainter {
    texts: Vec<String>,
    icons: Vec<String>,
    fills: Vec<(Rect, f32, Color)>,
    strokes: Vec<(Rect, f32, Color, f32)>,
    measure_calls: usize,
    max_measured_chars: usize,
}

impl Painter for CapturePainter {
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
        self.fills.push((rect, radius, color));
    }
    fn stroke_round_rect(&mut self, rect: Rect, radius: f32, color: Color, width: f32) {
        self.strokes.push((rect, radius, color, width));
    }
    fn stroke_svg_path(
        &mut self,
        d: &str,
        _top_left: Point2D,
        _size: f32,
        _color: Color,
        _width: f32,
    ) {
        self.icons.push(d.to_owned());
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
    fn measure_text_weighted(&mut self, text: &str, font_size: f32, _weight: u16) -> f32 {
        self.measure_calls += 1;
        self.max_measured_chars = self.max_measured_chars.max(text.chars().count());
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
}

#[test]
fn busy_composer_paints_stop_and_hovered_guide_tooltip() {
    let (mut state, session) = queue_state(1);
    state.transcripts.get_mut(&session).unwrap().busy = true;
    state.composer = ComposerState::default();
    let rect = Rect::xywh(0.0, 0.0, 736.0, 183.0);
    let queue = Composer::queue_layout(rect, &state).unwrap();
    let mut painter = CapturePainter::default();

    Composer::paint_input_with_app_context(
        &mut painter,
        rect,
        ComposerController::fixture("").input_state(),
        &state,
        Some(queue.rows[0].guide_id),
        &ZodeTheme::light(),
    );

    assert!(painter
        .icons
        .iter()
        .any(|path| path == SemanticIcon::Guide.path()));
    assert!(painter
        .texts
        .iter()
        .any(|text| text == "提交，但不中断模型运行"));
    let snapshot = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    assert_eq!(snapshot.node(SEND_ID).unwrap().name, "停止当前运行");
}

#[test]
fn dark_busy_composer_paints_a_theme_adaptive_solid_stop_square() {
    let (mut state, session) = queue_state(1);
    state.transcripts.get_mut(&session).unwrap().busy = true;
    state.composer = ComposerState::default();
    let rect = Rect::xywh(0.0, 0.0, 736.0, 183.0);
    let input = Composer::layout_for_state(rect, &state).input;
    let send = Rect::xywh(input.max_x() - 42.0, input.max_y() - 38.0, 28.0, 28.0);
    let stop = Rect::xywh(send.origin.x + 10.0, send.origin.y + 10.0, 8.0, 8.0);
    let theme = ZodeTheme::dark();
    let mut painter = CapturePainter::default();

    Composer::paint_input_with_app_context(
        &mut painter,
        rect,
        ComposerController::fixture("").input_state(),
        &state,
        None,
        &theme,
    );

    assert!(painter
        .fills
        .contains(&(send, 14.0, theme.tokens.foreground)));
    assert!(painter
        .fills
        .contains(&(stop, 1.5, theme.tokens.background)));
    assert!(!painter
        .icons
        .iter()
        .any(|path| path == SemanticIcon::Stop.path()));
    assert_eq!(center(send), center(stop));
}

#[test]
fn keyboard_focus_does_not_outline_queue_actions_or_menu_items() {
    let (mut state, _) = queue_state(2);
    state.composer.queue_menu = Some(QueuedMessageId::from_u64(1));
    let snapshot = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    let layout = Composer::queue_layout(snapshot.layout.composer, &state).expect("queue layout");
    let first = &layout.rows[0];
    let menu = layout.menu.as_ref().expect("open queue menu");
    let more_focus = Rect::xywh(
        first.more.origin.x + (first.more.size.x - 24.0) / 2.0,
        first.more.origin.y + (first.more.size.y - 24.0) / 2.0,
        24.0,
        24.0,
    );
    let theme = ZodeTheme::light();
    let terminal = TerminalGrid::new(80, 24);

    for (id, focus_rect, radius) in [
        (first.guide_id, first.guide, 7.0),
        (first.delete_id, first.delete, 7.0),
        (first.more_id, more_focus, 12.0),
        (menu.edit_id, menu.edit, 7.0),
        (menu.close_id, menu.close, 7.0),
    ] {
        let mut painter = CapturePainter::default();
        let mut focused_snapshot = snapshot.clone();
        focused_snapshot.focused = Some(id);
        WorkspaceShell::paint_snapshot_with_hovered_widget(
            &mut painter,
            &focused_snapshot,
            &state,
            ComposerController::fixture("").input_state(),
            &terminal,
            None,
            None,
            &theme,
        );

        assert!(
            !painter
                .strokes
                .contains(&(focus_rect, radius, theme.tokens.ring, 1.5)),
            "focused queue control {id:?} must not paint a purple outline"
        );
    }
}

#[test]
fn short_queue_only_emits_rows_that_fit_above_the_input() {
    let (state, _) = queue_state(4);
    let rect = Rect::xywh(0.0, 0.0, 736.0, 150.0);
    let stack = Composer::layout_for_state(rect, &state);
    let slot = stack.queue.expect("short queue slot");
    let queue = Composer::queue_layout(rect, &state).expect("one complete row");

    assert_eq!(queue.rows.len(), 1);
    assert!(queue.rows[0].rect.origin.y >= slot.origin.y);
    assert!(queue.rows[0].rect.max_y() <= slot.max_y());

    let too_short = Rect::xywh(0.0, 0.0, 736.0, 120.0);
    assert!(Composer::layout_for_state(too_short, &state)
        .queue
        .is_some());
    assert!(Composer::queue_layout(too_short, &state).is_none());
}

#[test]
fn ultra_narrow_queue_keeps_nonzero_disjoint_actions_and_more_operable() {
    let (state, _) = queue_state(4);
    let rect = Rect::xywh(0.0, 0.0, 64.0, 270.0);
    let queue = Composer::queue_layout(rect, &state).expect("compact queue");

    assert_eq!(queue.rows.len(), COMPOSER_QUEUE_MAX_VISIBLE);
    for row in &queue.rows {
        assert!(row.rect.size.x > 0.0);
        assert!(row.guide.size.x > 0.0);
        assert!(row.delete.size.x > 0.0);
        assert!(row.more.size.x >= 24.0);
        assert_eq!(row.guide.max_x(), row.delete.origin.x);
        assert_eq!(row.delete.max_x(), row.more.origin.x);
        assert_eq!(row.more.max_x(), row.rect.max_x());
        assert!(row.guide.origin.x >= row.rect.origin.x);
        assert!(row.more.max_x() <= row.rect.max_x());
    }
}

#[test]
fn tiny_workspace_hit_testing_and_accessibility_use_the_compact_queue_layout() {
    let (state, _) = queue_state(4);
    let snapshot = WorkspaceSnapshot::build(&state, 96.0, 400.0, Insets::ZERO);
    let queue = Composer::queue_layout(snapshot.layout.composer, &state).expect("compact queue");

    assert_eq!(snapshot.layout.composer.size.x, 64.0);
    for row in &queue.rows {
        for (id, rect) in [
            (row.guide_id, row.guide),
            (row.delete_id, row.delete),
            (row.more_id, row.more),
        ] {
            let node = snapshot.node(id).expect("visible compact action");
            assert_eq!(node.rect, rect);
            assert!(rect.size.x > 0.0);
            assert_eq!(snapshot.hit_test(center(rect)), Some(id));
        }
    }
}

#[test]
fn long_unicode_and_code_preview_is_bounded_and_ellipsized_logarithmically() {
    let (mut state, session) = queue_state(1);
    let long =
        "请🧪检查`cargo test --workspace && cargo clippy`这段超长代码与中文。".repeat(20_000);
    state.message_queues.get_mut(&session).unwrap().items[0].text = long;
    let rect = Rect::xywh(0.0, 0.0, 736.0, 183.0);
    let mut painter = CapturePainter::default();

    Composer::paint_input_with_app_context(
        &mut painter,
        rect,
        ComposerController::fixture("").input_state(),
        &state,
        None,
        &ZodeTheme::light(),
    );

    let preview = painter
        .texts
        .iter()
        .find(|text| text.starts_with("请🧪检查`cargo"))
        .expect("painted queue preview");
    assert!(preview.ends_with('…'));
    assert!(preview.is_char_boundary(preview.len()));
    assert!(preview.chars().count() < 512);
    assert!(painter.max_measured_chars <= 512);
    assert!(painter.measure_calls < 32);
}
