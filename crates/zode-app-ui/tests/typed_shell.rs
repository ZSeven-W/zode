use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    AppCommand, ComingSoonFeature, EnvironmentSnapshot, IntegrationsTab, LoadState, PreviewState,
    ProjectState, SecondaryPane, SessionDiffState, SessionPresentationState, SettingsCategory,
    ShellPage, ShellRoute,
};
use zode_app_ui::{
    EnvironmentPanel, Insets, PinnedSummaryMode, ReviewPanel, ThreadHeader, WorkspaceShell,
    ZodeTheme, HEADER_ENVIRONMENT_ID, HEADER_REVIEW_ID, PANEL_PICKER_ID, REVIEW_CLOSE_ID,
};
use zode_node_protocol::{
    DiffFile, DiffFileStatus, DiffSnapshot, SessionLocator, ThreadStatus, ThreadSummary,
    WorkspaceUri,
};

#[derive(Default)]
struct PaintCapture {
    texts: Vec<String>,
    svg_paths: Vec<String>,
    fills: Vec<Rect>,
    rounded_fills: Vec<Rect>,
    clips: Vec<Rect>,
}

impl Painter for PaintCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, rect: Rect, _color: Color) {
        self.fills.push(rect);
    }
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

fn state_with_ready_session() -> (zode_app_model::ZodeAppState, SessionLocator) {
    let mut state = zode_app_model::demo_state();
    let session = SessionLocator::new(state.host.node_id, "typed-shell");
    let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
    state.current_session = Some(session.clone());
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace_uri.clone(),
        title: "Zode 桌面端".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    state.projects.push(ProjectState {
        workspace_uri: workspace_uri.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.presentation.sessions.insert(
        session.clone(),
        SessionPresentationState {
            context: LoadState::Ready(EnvironmentSnapshot {
                workspace_uri,
                branch: Some("codex/typed-shell".into()),
                subagents: Vec::new(),
                background_processes: Vec::new(),
                sources: Vec::new(),
            }),
            diff: SessionDiffState {
                dirty: false,
                load: LoadState::Ready(ready_diff(&session)),
            },
            preview: PreviewState::Idle,
            runtime_options: LoadState::Idle,
        },
    );
    (state, session)
}

fn ready_diff(session: &SessionLocator) -> DiffSnapshot {
    DiffSnapshot {
        session: session.clone(),
        files: vec![DiffFile {
            path: "crates/zode-app-ui/src/widgets/workspace_shell.rs".into(),
            status: DiffFileStatus::Modified,
            additions: 7,
            deletions: 2,
        }],
        unified: concat!(
            "diff --git a/workspace_shell.rs b/workspace_shell.rs\n",
            "--- a/workspace_shell.rs\n",
            "+++ b/workspace_shell.rs\n",
            "@@ -1 +1 @@\n",
            "-legacy route\n",
            "+typed route\n",
        )
        .into(),
    }
}

fn paint_shell(
    state: &zode_app_model::ZodeAppState,
    width: f32,
) -> (PaintCapture, zode_app_ui::WorkspaceLayout) {
    let mut painter = PaintCapture::default();
    let layout = WorkspaceShell::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, width, 1_080.0),
        Insets::ZERO,
        state,
        &ZodeTheme::light(),
    );
    (painter, layout)
}

fn text(painter: &PaintCapture) -> String {
    painter.texts.join("\n")
}

#[test]
fn typed_settings_route_wins_over_stale_legacy_page() {
    let mut state = zode_app_model::demo_state();
    state.shell.page = ShellPage::Conversation;
    state.presentation.route = ShellRoute::Settings(SettingsCategory::General);

    let (painter, layout) = paint_shell(&state, 1_800.0);
    let text = text(&painter);

    assert_eq!(layout.page_content.size.x, 768.0);
    assert!(text.contains("常规"));
    assert!(text.contains("权限"));
    assert!(text.contains("只读"));
    assert!(!text.contains("我们应该构建什么？"));
    assert!(!text.contains("随心输入"));
}

#[test]
fn typed_integrations_and_coming_soon_routes_render_honest_pages() {
    let mut state = zode_app_model::demo_state();
    state.shell.page = ShellPage::Settings;
    state.presentation.route = ShellRoute::Integrations(IntegrationsTab::Plugins);

    let (plugins, _) = paint_shell(&state, 1_800.0);
    let plugins = text(&plugins);
    assert!(plugins.contains("插件"));
    assert!(plugins.contains("尚未加载本机集成"));
    assert!(!plugins.contains("安装"));
    assert!(!plugins.contains("常规"));

    state.presentation.route = ShellRoute::ComingSoon(ComingSoonFeature::PullRequests);
    let (coming_soon, _) = paint_shell(&state, 1_800.0);
    let coming_soon = text(&coming_soon);
    assert!(coming_soon.contains("拉取请求"));
    assert!(coming_soon.contains("即将支持"));
    assert!(!coming_soon.contains("0 个拉取请求"));
    assert!(!coming_soon.contains("随心输入"));
}

#[test]
fn typed_conversation_route_restores_chat_when_legacy_page_is_stale() {
    let mut state = zode_app_model::demo_state();
    state.shell.page = ShellPage::Settings;
    state.presentation.route = ShellRoute::Conversation;

    let (painter, _) = paint_shell(&state, 1_800.0);
    let text = text(&painter);

    assert!(text.contains("我们应该构建什么？"));
    assert!(text.contains("随心输入"));
    assert!(!text.contains("本地运行状态"));
}

#[test]
fn terminal_header_does_not_paint_unreachable_conversation_actions() {
    let (mut state, _) = state_with_ready_session();
    state.presentation.route = ShellRoute::Terminal;

    let (painter, _) = paint_shell(&state, 1_800.0);
    let text = text(&painter);

    assert!(text.contains("Zode 桌面端"));
    assert!(!painter.texts.iter().any(|text| text == "环境"));
    assert!(!painter.texts.iter().any(|text| text == "审查"));
}

#[test]
fn conversation_composer_projects_only_the_ready_current_session_branch() {
    let (mut state, session) = state_with_ready_session();
    state.presentation.route = ShellRoute::Conversation;

    let (ready, _) = paint_shell(&state, 1_800.0);
    assert!(text(&ready).contains("codex/typed-shell"));

    state
        .presentation
        .sessions
        .get_mut(&session)
        .unwrap()
        .context = LoadState::Loading;
    let (loading, _) = paint_shell(&state, 1_800.0);
    assert!(!text(&loading).contains("codex/typed-shell"));
    assert!(!text(&loading).lines().any(|line| line == "main"));
}

#[test]
fn new_task_composer_projects_the_latest_verified_active_workspace_branch() {
    let (mut state, session) = state_with_ready_session();
    let workspace = state.threads[0].workspace_uri.clone();
    state.active_workspace = Some(workspace.clone());
    state.current_session = None;
    state.presentation.route = ShellRoute::Conversation;

    let (ready, _) = paint_shell(&state, 1_800.0);
    let ready_text = text(&ready);
    assert!(ready_text.contains("zode"));
    assert!(ready_text.contains("本地"));
    assert!(ready_text.contains("codex/typed-shell"));
    assert!(ready
        .svg_paths
        .iter()
        .any(|path| path == zode_app_ui::SemanticIcon::Branch.path()));

    state
        .presentation
        .sessions
        .get_mut(&session)
        .unwrap()
        .context = LoadState::Ready(EnvironmentSnapshot {
        workspace_uri: WorkspaceUri::new("file:///repo/other").unwrap(),
        branch: Some("other-secret-branch".into()),
        subagents: Vec::new(),
        background_processes: Vec::new(),
        sources: Vec::new(),
    });
    let (mismatched, _) = paint_shell(&state, 1_800.0);
    let mismatched_text = text(&mismatched);
    assert!(!mismatched_text.contains("codex/typed-shell"));
    assert!(!mismatched_text.contains("other-secret-branch"));
    assert!(!mismatched_text.lines().any(|line| line == "main"));
    assert!(!mismatched
        .svg_paths
        .iter()
        .any(|path| path == zode_app_ui::SemanticIcon::Branch.path()));
}

#[test]
fn header_actions_are_stable_real_commands_and_picker_survives_without_a_session() {
    let (mut state, _) = state_with_ready_session();
    let rect = Rect::xywh(240.0, 0.0, 1_560.0, 46.0);

    let layout = ThreadHeader::layout(rect, &state);

    let environment = layout.environment.expect("environment action");
    let review = layout.review.expect("review action");
    let picker = layout.panel_picker.expect("panel picker");
    assert_eq!(environment.id, HEADER_ENVIRONMENT_ID);
    assert_eq!(environment.rect, Rect::xywh(1_684.0, 7.0, 32.0, 32.0));
    assert_eq!(review.id, HEADER_REVIEW_ID);
    assert_eq!(review.rect, Rect::xywh(1_720.0, 7.0, 32.0, 32.0));
    assert_eq!(picker.id, PANEL_PICKER_ID);
    assert_eq!(picker.rect, Rect::xywh(1_756.0, 7.0, 32.0, 32.0));
    assert_eq!(
        ThreadHeader::command_for_widget(&state, environment.id),
        Some(AppCommand::OpenSecondary(SecondaryPane::Environment)),
    );
    assert_eq!(
        ThreadHeader::command_for_widget(&state, review.id),
        Some(AppCommand::OpenReview),
    );

    state.presentation.secondary_pane = Some(SecondaryPane::Environment);
    let selected = ThreadHeader::layout(rect, &state);
    assert!(selected.environment.unwrap().selected);
    assert!(!selected.review.unwrap().selected);

    let narrow = ThreadHeader::layout(Rect::xywh(240.0, 0.0, 960.0, 46.0), &state);
    assert!(narrow.environment.is_some());
    assert_eq!(
        narrow.review.unwrap().rect,
        Rect::xywh(1_120.0, 7.0, 32.0, 32.0),
    );

    state.current_session = None;
    let empty = ThreadHeader::layout(rect, &state);
    assert!(empty.environment.is_none());
    assert!(empty.review.is_none());
    assert_eq!(empty.panel_picker.unwrap().id, PANEL_PICKER_ID);
    assert_eq!(
        ThreadHeader::command_for_widget(&state, HEADER_ENVIRONMENT_ID),
        None,
    );
    assert_eq!(
        ThreadHeader::command_for_widget(&state, HEADER_REVIEW_ID),
        None
    );
}

#[test]
fn environment_secondary_pane_paints_the_real_current_session_context() {
    let (mut state, _) = state_with_ready_session();
    state.presentation.route = ShellRoute::Conversation;
    state.presentation.secondary_pane = Some(SecondaryPane::Environment);

    let (painter, layout) = paint_shell(&state, 1_800.0);
    let text = text(&painter);

    assert_eq!(layout.context_panel.size.x, 300.0);
    assert!(text.contains("置顶摘要"));
    assert!(text.contains("环境信息"));
    assert!(text.contains("file:///repo/zode"));
    assert!(text.contains("codex/typed-shell"));
    assert!(painter
        .rounded_fills
        .contains(&EnvironmentPanel::layout(layout.context_panel, &state).card));
}

#[test]
fn pinned_summary_auto_docks_only_when_conversation_space_is_available() {
    let (mut state, _) = state_with_ready_session();
    state.presentation.route = ShellRoute::Conversation;
    state.presentation.secondary_pane = None;

    let (wide, wide_layout) = paint_shell(&state, 1_800.0);
    assert_eq!(wide_layout.pinned_summary, PinnedSummaryMode::Docked);
    assert_eq!(wide_layout.context_panel.size.x, 300.0);
    assert!(text(&wide).contains("置顶摘要"));
    assert!(
        ThreadHeader::layout_with_pinned_summary(
            wide_layout.top_bar,
            &state,
            wide_layout.pinned_summary,
        )
        .environment
        .unwrap()
        .selected
    );

    for width in [1_399.0, 1_250.0] {
        let (narrow, narrow_layout) = paint_shell(&state, width);
        assert_eq!(narrow_layout.pinned_summary, PinnedSummaryMode::Hidden);
        assert_eq!(narrow_layout.context_panel.size.x, 0.0);
        assert!(!text(&narrow).contains("置顶摘要"));
        assert!(
            !ThreadHeader::layout_with_pinned_summary(
                narrow_layout.top_bar,
                &state,
                narrow_layout.pinned_summary,
            )
            .environment
            .unwrap()
            .selected
        );
    }

    state.presentation.secondary_pane = Some(SecondaryPane::Environment);
    let (overlay, overlay_layout) = paint_shell(&state, 1_200.0);
    assert_eq!(overlay_layout.pinned_summary, PinnedSummaryMode::Overlay);
    assert_eq!(overlay_layout.context_panel.size.x, 300.0);
    assert!(text(&overlay).contains("置顶摘要"));
    assert!(
        ThreadHeader::layout_with_pinned_summary(
            overlay_layout.top_bar,
            &state,
            overlay_layout.pinned_summary,
        )
        .environment
        .unwrap()
        .selected
    );
}

#[test]
fn explicit_review_has_priority_over_the_automatic_summary() {
    let (mut state, _) = state_with_ready_session();
    state.presentation.route = ShellRoute::Conversation;
    state.presentation.secondary_pane = Some(SecondaryPane::Review);

    let (painter, layout) = paint_shell(&state, 1_800.0);

    assert_eq!(layout.pinned_summary, PinnedSummaryMode::Hidden);
    assert_eq!(layout.context_panel.size.x, 0.0);
    assert!(layout.review_panel.size.x > 0.0);
    assert!(!text(&painter).contains("置顶摘要"));
}

#[test]
fn wide_review_is_a_real_split_and_narrow_review_falls_back_to_the_primary_surface() {
    let (mut state, _) = state_with_ready_session();
    state.presentation.route = ShellRoute::Conversation;
    state.presentation.secondary_pane = Some(SecondaryPane::Review);

    let (wide, wide_layout) = paint_shell(&state, 1_800.0);
    let wide_text = text(&wide);
    assert_eq!(wide_layout.review_panel.size.x, 700.0);
    assert_eq!(wide_layout.divider.size.x, 1.0);
    assert!(wide_text.contains("Zode 桌面端"));
    assert!(wide_text.contains("变更"));
    assert!(wide_text.contains("workspace_shell.rs"));
    assert!(wide_text.contains("typed route"));

    let (narrow, narrow_layout) = paint_shell(&state, 1_200.0);
    let narrow_text = text(&narrow);
    assert_eq!(narrow_layout.review_panel.size.x, 0.0);
    assert!(narrow_text.contains("workspace_shell.rs"));
    assert!(narrow_text.contains("typed route"));
    assert!(!narrow_text.contains("我们应该构建什么？"));
    assert!(!narrow_text.contains("随心输入"));
    assert!(narrow.clips.contains(&narrow_layout.primary_surface));
}

#[test]
fn review_surface_exposes_all_real_load_states_and_a_stable_close_command() {
    let (mut state, session) = state_with_ready_session();
    let rect = Rect::xywh(1_100.0, 0.0, 700.0, 1_080.0);

    let layout = ReviewPanel::layout(rect);
    assert_eq!(layout.header, Rect::xywh(1_100.0, 0.0, 700.0, 46.0));
    assert_eq!(layout.close_button, Rect::xywh(1_764.0, 11.0, 24.0, 24.0));
    assert_eq!(
        ReviewPanel::command_for_widget(&state, REVIEW_CLOSE_ID),
        Some(AppCommand::CloseSecondary),
    );

    for (load, expected) in [
        (LoadState::Idle, "变更尚未加载"),
        (LoadState::Loading, "变更加载中"),
        (LoadState::Failed("offline".into()), "变更加载失败：offline"),
    ] {
        state
            .presentation
            .sessions
            .get_mut(&session)
            .unwrap()
            .diff
            .load = load;
        let mut painter = PaintCapture::default();
        ReviewPanel::paint_state(&mut painter, rect, &state, &ZodeTheme::light());
        assert!(text(&painter).contains(expected));
        assert!(!text(&painter).contains("workspace_shell.rs"));
    }

    state
        .presentation
        .sessions
        .get_mut(&session)
        .unwrap()
        .diff
        .load = LoadState::Ready(ready_diff(&session));
    let mut ready = PaintCapture::default();
    ReviewPanel::paint_state(&mut ready, rect, &state, &ZodeTheme::light());
    let ready_text = text(&ready);
    assert!(ready_text.contains("1 个文件"));
    assert!(ready_text.contains("+7 -2"));
    assert!(ready_text.contains("workspace_shell.rs"));
    assert!(ready_text.contains("typed route"));
    assert!(ready.clips.contains(&rect));

    state.presentation.sessions.remove(&session);
    let mut implicit_idle = PaintCapture::default();
    ReviewPanel::paint_state(&mut implicit_idle, rect, &state, &ZodeTheme::light());
    assert!(text(&implicit_idle).contains("变更尚未加载"));
    assert!(!text(&implicit_idle).contains("选择任务以查看变更"));

    state.current_session = None;
    let mut empty = PaintCapture::default();
    ReviewPanel::paint_state(&mut empty, rect, &state, &ZodeTheme::light());
    assert!(text(&empty).contains("选择任务以查看变更"));
    assert!(!text(&empty).contains("workspace_shell.rs"));
}
