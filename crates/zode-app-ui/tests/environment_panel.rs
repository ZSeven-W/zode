use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    AppCommand, ConnectionState, EnvironmentEntry, EnvironmentSnapshot, LoadState,
    SessionDiffState, SessionPresentationState,
};
use zode_app_ui::{
    EnvironmentPanel, RectExt, ZodeTheme, ENVIRONMENT_CLOSE_ID, ENVIRONMENT_REVIEW_ID,
};
use zode_node_protocol::{
    DiffFile, DiffFileStatus, DiffSnapshot, SessionLocator, ThreadStatus, ThreadSummary,
    WorkspaceUri,
};

#[derive(Default)]
struct PaintCapture {
    texts: Vec<String>,
    clips: Vec<Rect>,
    rounded_fills: Vec<Rect>,
    rounded_strokes: Vec<Rect>,
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
    fn clip_rect(&mut self, rect: Rect) {
        self.clips.push(rect);
    }
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, _radius: f32, _color: Color) {
        self.rounded_fills.push(rect);
    }
    fn stroke_round_rect(&mut self, rect: Rect, _radius: f32, _color: Color, _width: f32) {
        self.rounded_strokes.push(rect);
    }
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

fn state_with_session(id: &str) -> (zode_app_model::ZodeAppState, SessionLocator) {
    let mut state = zode_app_model::demo_state();
    let session = SessionLocator::new(state.host.node_id, id);
    let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
    state.current_session = Some(session.clone());
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri,
        title: "Zode desktop".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    (state, session)
}

fn ready_presentation(session: &SessionLocator) -> SessionPresentationState {
    SessionPresentationState {
        context: LoadState::Ready(EnvironmentSnapshot {
            workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
            branch: Some("codex/zode-desktop".into()),
            subagents: vec![EnvironmentEntry {
                id: "agent-1".into(),
                label: "界面实现".into(),
            }],
            background_processes: vec![EnvironmentEntry {
                id: "process-1".into(),
                label: "cargo test -p zode-app-ui".into(),
            }],
            sources: vec![EnvironmentEntry {
                id: "source-1".into(),
                label: "zode-superpowers.zip".into(),
            }],
        }),
        diff: SessionDiffState {
            dirty: false,
            load: LoadState::Ready(DiffSnapshot {
                session: session.clone(),
                files: vec![
                    DiffFile {
                        path: "crates/zode-app/src/main.rs".into(),
                        status: DiffFileStatus::Modified,
                        additions: 7,
                        deletions: 3,
                    },
                    DiffFile {
                        path: "README.md".into(),
                        status: DiffFileStatus::Modified,
                        additions: 3,
                        deletions: 1,
                    },
                ],
                unified: "diff --git a/README.md b/README.md".into(),
            }),
        },
    }
}

fn paint(state: &zode_app_model::ZodeAppState, rect: Rect) -> PaintCapture {
    let mut painter = PaintCapture::default();
    EnvironmentPanel::paint(&mut painter, rect, state, &ZodeTheme::light());
    painter
}

fn contains(outer: Rect, inner: Rect) -> bool {
    inner.min_x() >= outer.min_x()
        && inner.min_y() >= outer.min_y()
        && inner.max_x() <= outer.max_x()
        && inner.max_y() <= outer.max_y()
}

#[test]
fn wide_layout_is_a_300px_floating_card_with_stable_real_commands() {
    let (mut state, session) = state_with_session("current");
    state.presentation.sessions.insert(
        session,
        ready_presentation(state.current_session.as_ref().unwrap()),
    );
    let surface = Rect::xywh(1_484.0, 62.0, 300.0, 1_002.0);

    let layout = EnvironmentPanel::layout(surface, &state);

    assert_eq!(layout.card.origin, surface.origin);
    assert_eq!(layout.card.size.x, 300.0);
    assert_eq!(layout.card.size.y, 512.0);
    assert_eq!(layout.close_button, Rect::xywh(1_752.0, 74.0, 20.0, 20.0));
    assert_eq!(
        layout.review_button,
        Some(Rect::xywh(1_500.0, 524.0, 268.0, 34.0)),
    );
    assert_eq!(
        EnvironmentPanel::command_for_widget(&state, ENVIRONMENT_CLOSE_ID),
        Some(AppCommand::CloseSecondary),
    );
    assert_eq!(
        EnvironmentPanel::command_for_widget(&state, ENVIRONMENT_REVIEW_ID),
        Some(AppCommand::OpenReview),
    );
}

#[test]
fn no_session_keeps_host_truth_and_prompts_for_a_task() {
    let mut state = zode_app_model::demo_state();
    state.host.connection = ConnectionState::Connecting;

    let painter = paint(&state, Rect::xywh(100.0, 50.0, 300.0, 500.0));
    let text = painter.texts.join("\n");

    assert!(text.contains("环境信息"));
    assert!(text.contains("主机连接"));
    assert!(text.contains("连接中"));
    assert!(text.contains("选择任务以查看环境"));
    assert_eq!(
        EnvironmentPanel::command_for_widget(&state, ENVIRONMENT_REVIEW_ID),
        None,
    );
}

#[test]
fn current_session_load_states_are_explicit_and_keep_the_real_workspace() {
    let (mut state, session) = state_with_session("current");

    for (context, expected) in [
        (LoadState::Idle, "尚未加载"),
        (LoadState::Loading, "加载中"),
        (
            LoadState::Failed("git 不可用".into()),
            "加载失败：git 不可用",
        ),
    ] {
        state.presentation.sessions.insert(
            session.clone(),
            SessionPresentationState {
                context,
                diff: SessionDiffState::default(),
            },
        );
        let text = paint(&state, Rect::xywh(0.0, 0.0, 300.0, 600.0))
            .texts
            .join("\n");
        assert!(text.contains("当前工作区"));
        assert!(text.contains("file:///repo/zode"));
        assert!(text.contains(expected), "missing context state: {expected}");
    }
}

#[test]
fn ready_context_and_diff_project_only_real_non_empty_data() {
    let (mut state, session) = state_with_session("current");
    state
        .presentation
        .sessions
        .insert(session.clone(), ready_presentation(&session));

    let text = paint(&state, Rect::xywh(0.0, 0.0, 300.0, 700.0))
        .texts
        .join("\n");

    for expected in [
        "已就绪",
        "codex/zode-desktop",
        "子智能体",
        "界面实现",
        "后台进程",
        "cargo test -p zode-app-ui",
        "来源",
        "zode-superpowers.zip",
        "2 个文件",
        "+10 -4",
        "查看变更",
    ] {
        assert!(text.contains(expected), "missing real value: {expected}");
    }
    for fabricated in ["51 完成", "main", "网页搜索", "查看全部"] {
        assert!(!text.contains(fabricated), "fabricated value: {fabricated}");
    }

    state.presentation.sessions.insert(
        session.clone(),
        SessionPresentationState {
            context: LoadState::Ready(EnvironmentSnapshot {
                workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
                branch: None,
                subagents: Vec::new(),
                background_processes: Vec::new(),
                sources: Vec::new(),
            }),
            diff: SessionDiffState {
                dirty: false,
                load: LoadState::Ready(DiffSnapshot {
                    session,
                    files: Vec::new(),
                    unified: String::new(),
                }),
            },
        },
    );
    let empty_ready = paint(&state, Rect::xywh(0.0, 0.0, 300.0, 700.0))
        .texts
        .join("\n");
    for absent in ["分支", "子智能体", "后台进程", "来源"] {
        assert!(
            !empty_ready.contains(absent),
            "empty section leaked: {absent}"
        );
    }
    assert!(empty_ready.contains("0 个文件"));
}

#[test]
fn diff_idle_loading_and_failure_are_honest_and_do_not_open_review() {
    let (mut state, session) = state_with_session("current");

    for (load, expected) in [
        (LoadState::Idle, "变更尚未加载"),
        (LoadState::Loading, "变更加载中"),
        (
            LoadState::Failed("endpoint offline".into()),
            "变更加载失败：endpoint offline",
        ),
    ] {
        state.presentation.sessions.insert(
            session.clone(),
            SessionPresentationState {
                context: LoadState::Idle,
                diff: SessionDiffState { dirty: true, load },
            },
        );
        let painter = paint(&state, Rect::xywh(0.0, 0.0, 300.0, 600.0));
        assert!(painter.texts.join("\n").contains(expected));
        assert_eq!(
            EnvironmentPanel::layout(Rect::xywh(0.0, 0.0, 300.0, 600.0), &state).review_button,
            None
        );
        assert_eq!(
            EnvironmentPanel::command_for_widget(&state, ENVIRONMENT_REVIEW_ID),
            None,
        );
    }
}

#[test]
fn presentation_data_never_leaks_from_another_session() {
    let (mut state, current) = state_with_session("current");
    let other = SessionLocator::new(state.host.node_id, "other");
    let mut current_presentation = ready_presentation(&current);
    if let LoadState::Ready(context) = &mut current_presentation.context {
        context.branch = Some("current-branch".into());
    }
    let mut other_presentation = ready_presentation(&other);
    if let LoadState::Ready(context) = &mut other_presentation.context {
        context.branch = Some("other-secret-branch".into());
        context.sources[0].label = "other-secret-source".into();
    }
    state
        .presentation
        .sessions
        .insert(current, current_presentation);
    state
        .presentation
        .sessions
        .insert(other, other_presentation);

    let text = paint(&state, Rect::xywh(0.0, 0.0, 300.0, 700.0))
        .texts
        .join("\n");

    assert!(text.contains("current-branch"));
    assert!(!text.contains("other-secret-branch"));
    assert!(!text.contains("other-secret-source"));
}

#[test]
fn narrow_short_surfaces_clip_and_keep_all_interaction_rects_contained() {
    let (mut state, session) = state_with_session("current");
    state
        .presentation
        .sessions
        .insert(session.clone(), ready_presentation(&session));
    let surface = Rect::xywh(17.0, 23.0, 120.0, 90.0);

    let layout = EnvironmentPanel::layout(surface, &state);
    let painter = paint(&state, surface);

    assert_eq!(layout.card, surface);
    assert!(contains(layout.card, layout.header));
    assert!(contains(layout.card, layout.close_button));
    assert_eq!(layout.review_button, None);
    assert!(painter.clips.contains(&layout.card));
    assert!(painter.rounded_fills.contains(&layout.card));
    assert!(painter.rounded_strokes.contains(&layout.card));
}
