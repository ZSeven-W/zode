use accesskit::{Action, Role};
use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{AppCommand, ProjectState, SessionRenameState};
use zode_app_ui::{
    Insets, RectExt, SemanticIcon, ThreadHeader, WorkspaceSnapshot, ZodeTheme,
    HEADER_COPY_TITLE_ID, HEADER_ENVIRONMENT_ID, HEADER_MENU_COPY_ID, HEADER_MENU_NEW_WINDOW_ID,
    HEADER_MENU_RENAME_ID, HEADER_RENAME_INPUT_ID, HEADER_RENAME_SAVE_ID,
};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

#[derive(Default)]
struct PaintCapture {
    texts: Vec<String>,
    paths: Vec<String>,
    rounded_fills: Vec<Rect>,
    rounded_strokes: Vec<(Rect, Color, f32)>,
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
    fn fill_round_rect(&mut self, rect: Rect, _radius: f32, _color: Color) {
        self.rounded_fills.push(rect);
    }
    fn stroke_round_rect(&mut self, rect: Rect, _radius: f32, color: Color, width: f32) {
        self.rounded_strokes.push((rect, color, width));
    }
    fn stroke_svg_path(
        &mut self,
        path: &str,
        _top_left: Point2D,
        _size: f32,
        _color: Color,
        _width: f32,
    ) {
        self.paths.push(path.into());
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

fn state_with_session() -> (zode_app_model::ZodeAppState, SessionLocator) {
    let mut state = zode_app_model::demo_state();
    let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
    let session = SessionLocator::new(state.host.node_id, "header-menu");
    state.projects.push(ProjectState {
        workspace_uri: workspace_uri.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri,
        title: "zode 桌面端".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    state.current_session = Some(session.clone());
    (state, session)
}

#[test]
fn title_more_button_opens_real_pin_and_archive_commands() {
    let (mut state, session) = state_with_session();
    let rect = Rect::xywh(240.0, 0.0, 1_560.0, 46.0);
    let header = ThreadHeader::layout(rect, &state);
    let more = header.more.expect("current task exposes its action menu");

    assert!(more.rect.origin.x >= header.title.max_x());
    assert_eq!(
        ThreadHeader::command_for_widget(&state, more.id),
        Some(AppCommand::ToggleSessionMenu {
            session: session.clone(),
        })
    );

    state.session_menu = Some(session.clone());
    let menu = ThreadHeader::menu_layout(rect, &state).expect("open task menu");
    assert_eq!(
        ThreadHeader::command_for_widget(&state, menu.pin.id),
        Some(AppCommand::SetSessionPinned {
            session: session.clone(),
            pinned: true,
        })
    );
    assert_eq!(
        ThreadHeader::command_for_widget(&state, menu.archive.id),
        Some(AppCommand::SetSessionArchived {
            session,
            archived: true,
        })
    );
}

#[test]
fn pinned_summary_toggle_paints_its_product_tooltip() {
    let (state, _) = state_with_session();
    let rect = Rect::xywh(240.0, 0.0, 1_560.0, 46.0);
    let mut painter = PaintCapture::default();

    ThreadHeader::paint_overlays(
        &mut painter,
        rect,
        Rect::xywh(0.0, 0.0, 1_800.0, 1_080.0),
        &state,
        None,
        Some(HEADER_ENVIRONMENT_ID),
        &ZodeTheme::light(),
    );

    assert!(painter.texts.iter().any(|text| text == "切换置顶摘要"));
}

#[test]
fn open_menu_uses_overlay_hit_geometry_and_menu_accessibility_roles() {
    let (mut state, session) = state_with_session();
    state.session_menu = Some(session);
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let menu = ThreadHeader::menu_layout(snapshot.layout.top_bar, &state).unwrap();

    let menu_node = snapshot.node(menu.id).expect("menu semantics");
    assert_eq!(menu_node.role, Role::Menu);
    assert_eq!(menu_node.name, "任务操作");
    assert_eq!(snapshot.node(menu.pin.id).unwrap().role, Role::MenuItem);
    assert_eq!(snapshot.node(menu.pin.id).unwrap().name, "置顶任务");
    assert_eq!(snapshot.node(menu.archive.id).unwrap().role, Role::MenuItem);
    assert_eq!(snapshot.node(menu.archive.id).unwrap().name, "归档任务");

    let pin_center = Point2D::new(
        menu.pin.rect.origin.x + menu.pin.rect.size.x / 2.0,
        menu.pin.rect.origin.y + menu.pin.rect.size.y / 2.0,
    );
    assert_eq!(snapshot.hit_test(pin_center), Some(menu.pin.id));
}

#[test]
fn task_menu_paints_native_icons_and_reflects_pinned_state() {
    let (mut state, session) = state_with_session();
    state.session_menu = Some(session.clone());
    state.pinned_sessions.insert(session);
    let rect = Rect::xywh(240.0, 0.0, 1_560.0, 46.0);
    let menu = ThreadHeader::menu_layout(rect, &state).unwrap();
    let mut painter = PaintCapture::default();

    ThreadHeader::paint_overlays(
        &mut painter,
        rect,
        Rect::xywh(0.0, 0.0, 1_800.0, 1_080.0),
        &state,
        None,
        Some(menu.pin.id),
        &ZodeTheme::light(),
    );

    assert!(painter.texts.iter().any(|text| text == "取消置顶"));
    assert!(painter.texts.iter().any(|text| text == "归档任务"));
    assert!(painter
        .paths
        .iter()
        .any(|path| path == SemanticIcon::Pin.path()));
    assert!(painter
        .paths
        .iter()
        .any(|path| path == SemanticIcon::Archive.path()));
    assert!(painter.rounded_fills.contains(&menu.rect));
    assert!(painter.rounded_fills.contains(&menu.pin.rect));
}

#[test]
fn full_task_menu_maps_real_actions_and_keeps_future_rows_disabled() {
    let (mut state, session) = state_with_session();
    state.session_menu = Some(session.clone());
    let rect = Rect::xywh(240.0, 0.0, 1_560.0, 46.0);
    let menu = ThreadHeader::menu_layout(rect, &state).unwrap();

    assert_eq!(menu.rename.id, HEADER_MENU_RENAME_ID);
    assert_eq!(menu.copy.id, HEADER_MENU_COPY_ID);
    assert_eq!(menu.new_window.id, HEADER_MENU_NEW_WINDOW_ID);
    assert_eq!(
        ThreadHeader::command_for_widget(&state, menu.rename.id),
        Some(AppCommand::BeginRenameSession {
            session: session.clone()
        })
    );
    assert_eq!(
        ThreadHeader::command_for_widget(&state, menu.copy.id),
        Some(AppCommand::ToggleSessionCopyMenu {
            session: session.clone()
        })
    );
    assert_eq!(
        ThreadHeader::command_for_widget(&state, menu.new_window.id),
        Some(AppCommand::OpenSessionInNewWindow { session })
    );
    for disabled in [menu.side_task, menu.continue_in, menu.schedule] {
        assert!(!disabled.enabled);
        assert_eq!(ThreadHeader::command_for_widget(&state, disabled.id), None);
    }
}

#[test]
fn copy_submenu_exposes_title_details_and_id_commands() {
    let (mut state, session) = state_with_session();
    state.session_menu = Some(session.clone());
    state.session_copy_menu = Some(session.clone());
    let rect = Rect::xywh(240.0, 0.0, 1_560.0, 46.0);
    let copy = ThreadHeader::menu_layout(rect, &state)
        .unwrap()
        .copy_menu
        .unwrap();

    assert_eq!(copy.title.id, HEADER_COPY_TITLE_ID);
    assert_eq!(
        ThreadHeader::command_for_widget(&state, copy.title.id),
        Some(AppCommand::CopyText("zode 桌面端".into()))
    );
    let AppCommand::CopyText(details) =
        ThreadHeader::command_for_widget(&state, copy.details.id).unwrap()
    else {
        panic!("copy details must be a clipboard command")
    };
    assert!(details.contains("项目：zode"));
    assert!(details.contains(&session.session_id));
    assert_eq!(
        ThreadHeader::command_for_widget(&state, copy.session_id.id),
        Some(AppCommand::CopyText(session.session_id))
    );
}

#[test]
fn disabled_rows_are_semantic_but_not_focusable_or_hittable() {
    let (mut state, session) = state_with_session();
    state.session_menu = Some(session);
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let menu = ThreadHeader::menu_layout(snapshot.layout.top_bar, &state).unwrap();

    for disabled in [menu.side_task, menu.continue_in, menu.schedule] {
        let node = snapshot.node(disabled.id).unwrap();
        assert!(node.disabled);
        assert!(node.actions.is_empty());
        assert_eq!(node.focus_order, None);
        assert_eq!(
            snapshot.hit_test(Point2D::new(
                disabled.rect.origin.x + disabled.rect.size.x / 2.0,
                disabled.rect.origin.y + disabled.rect.size.y / 2.0,
            )),
            None
        );
    }
}

#[test]
fn rename_dialog_focuses_an_editable_accessible_input_and_disables_blank_save() {
    let (mut state, session) = state_with_session();
    state.session_rename = Some(SessionRenameState {
        session: session.clone(),
        draft: "zode 桌面端".into(),
    });
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    assert_eq!(snapshot.focused, Some(HEADER_RENAME_INPUT_ID));
    let input = snapshot.node(HEADER_RENAME_INPUT_ID).unwrap();
    assert_eq!(input.role, Role::TextInput);
    assert_eq!(input.value.as_deref(), Some("zode 桌面端"));
    assert!(input.actions.contains(&Action::SetValue));

    let theme = ZodeTheme::light();
    let rename = ThreadHeader::rename_layout(Rect::xywh(240.0, 0.0, 1_560.0, 46.0), &state)
        .expect("rename layout");
    let mut painter = PaintCapture::default();
    ThreadHeader::paint_overlays(
        &mut painter,
        Rect::xywh(240.0, 0.0, 1_560.0, 46.0),
        Rect::xywh(0.0, 0.0, 1_800.0, 1_080.0),
        &state,
        Some(HEADER_RENAME_INPUT_ID),
        None,
        &theme,
    );
    assert!(
        painter
            .rounded_strokes
            .contains(&(rename.input, theme.tokens.ring, 1.5)),
        "focused text inputs must retain their focus border"
    );

    state.session_rename = Some(SessionRenameState {
        session,
        draft: "   ".into(),
    });
    let blank = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    assert!(blank.node(HEADER_RENAME_SAVE_ID).unwrap().disabled);
    assert!(blank
        .node(HEADER_RENAME_SAVE_ID)
        .unwrap()
        .actions
        .is_empty());
}
