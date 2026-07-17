use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    AppCommand, ConnectionState, EnvironmentActionKind, EnvironmentEntry, EnvironmentSectionKind,
    EnvironmentSnapshot, FileArtifact, LoadState, PreviewState, ProjectState, SessionDiffState,
    SessionPresentationState, TranscriptItem, TranscriptState,
};
use zode_app_ui::{
    EnvironmentPanel, RectExt, ZodeTheme, DOCUMENT_PREVIEW_CLOSE_ID, DOCUMENT_PREVIEW_EXTERNAL_ID,
    DOCUMENT_PREVIEW_RETRY_ID, ENVIRONMENT_CLOSE_ID, ENVIRONMENT_COMMIT_PUSH_ID,
    ENVIRONMENT_OPEN_WORKSPACE_ID, ENVIRONMENT_REFRESH_ID, ENVIRONMENT_REVIEW_ID,
};
use zode_node_protocol::{
    DiffFile, DiffFileStatus, DiffSnapshot, SessionLocator, ThreadStatus, ThreadSummary,
    WorkspaceUri,
};

#[derive(Default)]
struct PaintCapture {
    texts: Vec<String>,
    text_origins: Vec<(String, Point2D)>,
    clips: Vec<Rect>,
    rounded_fills: Vec<Rect>,
    rounded_strokes: Vec<Rect>,
    shadows: Vec<(Rect, f32, f32)>,
}

impl Painter for PaintCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, origin: Point2D) {
        let text: String = layout
            .runs()
            .iter()
            .map(|run| run.content.as_str())
            .collect();
        self.texts.push(text.clone());
        self.text_origins.push((text, origin));
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
    fn fill_drop_shadow(&mut self, rect: Rect, radius: f32, blur: f32, _color: Color) {
        self.shadows.push((rect, radius, blur));
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
    state.projects.push(ProjectState {
        workspace_uri: workspace_uri.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri,
        title: "Zode desktop".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    state
        .transcripts
        .insert(session.clone(), TranscriptState::default());
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
                value: None,
            }],
            background_processes: vec![EnvironmentEntry {
                id: "process-1".into(),
                label: "cargo test -p zode-app-ui".into(),
                value: None,
            }],
            sources: vec![EnvironmentEntry {
                id: "source-1".into(),
                label: "zode-superpowers.zip".into(),
                value: None,
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
        preview: PreviewState::Idle,
        runtime_options: LoadState::Idle,
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
fn wide_layout_is_a_300px_content_hug_card_with_stable_real_commands() {
    let (mut state, session) = state_with_session("current");
    state.presentation.sessions.insert(
        session,
        ready_presentation(state.current_session.as_ref().unwrap()),
    );
    let surface = Rect::xywh(1_484.0, 62.0, 300.0, 1_002.0);

    let layout = EnvironmentPanel::layout(surface, &state);

    assert_eq!(layout.card.origin, surface.origin);
    assert_eq!(layout.card.size.x, 300.0);
    assert!((320.0..=512.0).contains(&layout.card.size.y));
    assert_eq!(layout.close_button, Rect::xywh(1_752.0, 74.0, 20.0, 20.0));
    let review = layout
        .review_button
        .expect("a non-empty diff is reviewable");
    assert_eq!(review.origin.x, 1_500.0);
    assert_eq!(review.size.x, 268.0);
    assert_eq!(review.size.y, 34.0);
    assert_eq!(layout.repository_actions.len(), 4);
    assert_eq!(layout.repository_actions[1].rect, review);
    assert_eq!(
        EnvironmentPanel::command_for_widget(&state, ENVIRONMENT_CLOSE_ID),
        Some(AppCommand::CloseSecondary),
    );
    assert_eq!(
        EnvironmentPanel::command_for_widget(&state, ENVIRONMENT_REVIEW_ID),
        Some(AppCommand::RunEnvironmentAction {
            session: state.current_session.clone().unwrap(),
            action: EnvironmentActionKind::CompareWorkspaceToHead,
        }),
    );
    assert_eq!(
        EnvironmentPanel::command_for_widget(&state, ENVIRONMENT_REFRESH_ID),
        Some(AppCommand::RunEnvironmentAction {
            session: state.current_session.clone().unwrap(),
            action: EnvironmentActionKind::RefreshStatus,
        }),
    );
    assert_eq!(
        EnvironmentPanel::command_for_widget(&state, ENVIRONMENT_OPEN_WORKSPACE_ID),
        Some(AppCommand::RunEnvironmentAction {
            session: state.current_session.clone().unwrap(),
            action: EnvironmentActionKind::OpenWorkspace,
        }),
    );
    assert_eq!(
        EnvironmentPanel::command_for_widget(&state, ENVIRONMENT_COMMIT_PUSH_ID),
        None,
    );
}

#[test]
fn environment_action_ids_are_disjoint_from_document_preview_controls() {
    let environment_ids = [
        ENVIRONMENT_REFRESH_ID,
        ENVIRONMENT_OPEN_WORKSPACE_ID,
        ENVIRONMENT_COMMIT_PUSH_ID,
    ];
    let document_ids = [
        DOCUMENT_PREVIEW_CLOSE_ID,
        DOCUMENT_PREVIEW_EXTERNAL_ID,
        DOCUMENT_PREVIEW_RETRY_ID,
    ];

    for id in environment_ids {
        assert!(!document_ids.contains(&id), "duplicate widget id: {id:?}");
    }
}

#[test]
fn floating_panel_paints_one_soft_shadow_behind_the_card() {
    let state = zode_app_model::demo_state();
    let surface = Rect::xywh(1_484.0, 62.0, 300.0, 1_002.0);
    let layout = EnvironmentPanel::layout(surface, &state);

    let painter = paint(&state, surface);

    assert_eq!(painter.shadows.len(), 1);
    let (shadow, radius, blur) = painter.shadows[0];
    assert_eq!(shadow.origin.x, layout.card.origin.x);
    assert_eq!(shadow.origin.y, layout.card.origin.y + 4.0);
    assert_eq!(shadow.size, layout.card.size);
    assert_eq!(radius, 16.0);
    assert_eq!(blur, 24.0);
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
                preview: PreviewState::Idle,
                runtime_options: LoadState::Idle,
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
    state
        .transcripts
        .entry(session.clone())
        .or_default()
        .items
        .push(TranscriptItem::FileArtifact(FileArtifact {
            id: "artifact".into(),
            path: "docs/report.md".into(),
            summary: "报告".into(),
            change_summary: None,
        }));

    let painter = paint(&state, Rect::xywh(0.0, 0.0, 300.0, 700.0));
    let text = painter.texts.join("\n");

    for expected in [
        "codex/zode-desktop",
        "子智能体",
        "界面实现",
        "后台进程",
        "cargo test -p zode-app-ui",
        "来源",
        "docs/report.md",
        "2 个文件",
        "+10 -4",
        "比较工作区与 HEAD",
        "没有安全写入契约",
    ] {
        assert!(text.contains(expected), "missing real value: {expected}");
    }
    for fabricated in [
        "已就绪",
        "zode-superpowers.zip",
        "51 完成",
        "main",
        "网页搜索",
        "查看全部",
    ] {
        assert!(!text.contains(fabricated), "fabricated value: {fabricated}");
    }
    let text_y = |needle: &str| {
        painter
            .text_origins
            .iter()
            .find_map(|(text, origin)| (text == needle).then_some(origin.y))
            .expect("text is painted")
    };
    assert!(text_y("变更") < text_y("本地"));
    assert!(text_y("本地") < text_y("分支"));
    assert!(
        (text_y("当前工作区") - text_y("file:///repo/zode")).abs() <= 0.5,
        "the label and value must share one vertically centered row",
    );

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
                    session: session.clone(),
                    files: Vec::new(),
                    unified: String::new(),
                }),
            },
            preview: PreviewState::Idle,
            runtime_options: LoadState::Idle,
        },
    );
    state.transcripts.entry(session).or_default().items.clear();
    let empty_ready = paint(&state, Rect::xywh(0.0, 0.0, 300.0, 700.0))
        .texts
        .join("\n");
    for absent in [
        "文件变更",
        "当前分支",
        "比较分支",
        "子智能体",
        "后台进程",
        "来源",
        "0 个文件",
    ] {
        assert!(
            !empty_ready.contains(absent),
            "empty section leaked: {absent}"
        );
    }
    assert_eq!(
        EnvironmentPanel::command_for_widget(&state, ENVIRONMENT_REVIEW_ID),
        None,
    );
}

#[test]
fn long_environment_values_use_middle_ellipsis_but_keep_full_semantics() {
    let (mut state, session) = state_with_session("long-workspace");
    let workspace = "file:///Users/fini/workspace/z-seven/zode/.worktrees/zode-jian-desktop/crates/zode-app-ui/src/widgets/environment/mod.rs";
    let mut presentation = ready_presentation(&session);
    let LoadState::Ready(context) = &mut presentation.context else {
        unreachable!("ready fixture context")
    };
    context.workspace_uri = WorkspaceUri::new(workspace).unwrap();
    state.presentation.sessions.insert(session, presentation);

    let surface = Rect::xywh(0.0, 0.0, 300.0, 700.0);
    let layout = EnvironmentPanel::layout(surface, &state);
    let painter = paint(&state, surface);

    let visible = painter
        .texts
        .iter()
        .find(|text| text.contains('…') && text.starts_with("file") && text.ends_with("mod.rs"))
        .expect("long path keeps its identity around a middle ellipsis");
    assert_ne!(visible, workspace);
    let host = layout
        .sections
        .iter()
        .find(|section| section.section.kind == EnvironmentSectionKind::Host)
        .expect("host section");
    assert!(EnvironmentPanel::section_accessibility_name(host).contains(workspace));
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
                preview: PreviewState::Idle,
                runtime_options: LoadState::Idle,
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

#[test]
fn real_sections_drive_content_hug_geometry_and_footer_availability() {
    let (mut state, session) = state_with_session("current");
    state
        .presentation
        .sessions
        .insert(session.clone(), ready_presentation(&session));
    state
        .transcripts
        .entry(session.clone())
        .or_default()
        .items
        .push(TranscriptItem::FileArtifact(FileArtifact {
            id: "artifact".into(),
            path: "docs/report.md".into(),
            summary: "报告".into(),
            change_summary: None,
        }));

    let layout = EnvironmentPanel::layout(Rect::xywh(0.0, 0.0, 300.0, 900.0), &state);
    let kinds = layout
        .sections
        .iter()
        .map(|layout| layout.section.kind)
        .collect::<Vec<_>>();

    assert_eq!(
        kinds,
        vec![
            EnvironmentSectionKind::Changes,
            EnvironmentSectionKind::Host,
            EnvironmentSectionKind::Branch,
            EnvironmentSectionKind::RepositoryActions,
            EnvironmentSectionKind::Comparisons,
            EnvironmentSectionKind::Subagents,
            EnvironmentSectionKind::BackgroundProcesses,
            EnvironmentSectionKind::Sources,
        ]
    );
    assert!(layout
        .sections
        .iter()
        .all(|section| !section.section.entries.is_empty()));
    assert!((320.0..=512.0).contains(&layout.card.size.y));
    let last_row = layout.last_row.expect("a real section has a final row");
    assert!(layout.card.max_y() - last_row.max_y() <= 24.0);
    assert!(layout.review_button.is_some());
    assert!(layout
        .review_button
        .is_none_or(|review| layout.content.max_y() <= review.origin.y - 8.0));

    if let LoadState::Ready(context) = &mut state
        .presentation
        .sessions
        .get_mut(&session)
        .expect("current session presentation")
        .context
    {
        context
            .subagents
            .extend((0..20).map(|index| EnvironmentEntry {
                id: format!("overflow-agent-{index}"),
                label: format!("overflow agent {index}"),
                value: None,
            }));
    }
    let overflow_surface = Rect::xywh(0.0, 0.0, 300.0, 900.0);
    let overflow = EnvironmentPanel::layout(overflow_surface, &state);
    let overflow_paint = paint(&state, overflow_surface);
    let review = overflow
        .review_button
        .expect("diff footer remains available");
    assert_eq!(overflow.card.size.y, 512.0);
    assert!(overflow.content.max_y() <= review.origin.y - 8.0);
    assert!(overflow_paint.clips.contains(&overflow.content));

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
                    session: session.clone(),
                    files: Vec::new(),
                    unified: String::new(),
                }),
            },
            preview: PreviewState::Idle,
            runtime_options: LoadState::Idle,
        },
    );
    state.transcripts.entry(session).or_default().items.clear();
    let reduced = EnvironmentPanel::layout(Rect::xywh(0.0, 0.0, 300.0, 900.0), &state);

    assert!(reduced.card.size.y < layout.card.size.y);
    assert_eq!(reduced.card.size.y, 320.0);
    assert_eq!(
        reduced
            .sections
            .iter()
            .map(|section| section.section.kind)
            .collect::<Vec<_>>(),
        vec![EnvironmentSectionKind::Host]
    );
    assert!(reduced.review_button.is_none());
    let reduced_last_row = reduced.last_row.expect("host has a final real row");
    assert!(reduced.card.max_y() - reduced_last_row.max_y() <= 24.0);
}
