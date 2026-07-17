use accesskit::{Action, Role};
use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    demo_state, AppCommand, AttachmentMetadata, FileArtifact, LoadState, PreviewKind, PreviewState,
    PreviewTarget, ProjectState, SecondaryPane, SessionDiffState, SessionPresentationState,
    ShellRoute, TranscriptItem, TranscriptState,
};
use zode_app_ui::{
    DocumentPreview, Insets, RectExt, ReviewPanel, SemanticIcon, ThreadTranscript, WorkspaceLayout,
    WorkspaceSnapshot, ZodeTheme, DOCUMENT_PREVIEW_CLOSE_ID, DOCUMENT_PREVIEW_CONTENT_ID,
};
use zode_node_protocol::{
    DiffFile, DiffFileStatus, DiffSnapshot, SessionLocator, ThreadStatus, ThreadSummary,
    WorkspaceUri,
};

fn state_with_session() -> (zode_app_model::ZodeAppState, SessionLocator, WorkspaceUri) {
    let mut state = demo_state();
    let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
    let session = SessionLocator::new(state.host.node_id, "preview");
    state.projects.push(ProjectState {
        workspace_uri: workspace_uri.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace_uri.clone(),
        title: "preview".into(),
        updated_at_ms: 0,
        status: ThreadStatus::Idle,
    });
    state.transcripts.insert(
        session.clone(),
        TranscriptState {
            follow_tail: false,
            ..TranscriptState::default()
        },
    );
    state.current_session = Some(session.clone());
    state.active_workspace = Some(workspace_uri.clone());
    (state, session, workspace_uri)
}

#[test]
fn path_backed_transcript_items_share_button_hit_a11y_and_command_geometry() {
    let (mut state, session, _) = state_with_session();
    state.transcripts.get_mut(&session).unwrap().items = vec![
        TranscriptItem::FileArtifact(FileArtifact {
            id: "artifact".into(),
            path: "docs/report.md".into(),
            summary: "Report".into(),
            change_summary: None,
        }),
        TranscriptItem::Attachment(AttachmentMetadata {
            id: "file-attachment".into(),
            path: Some("notes.txt".into()),
            display_name: "notes.txt".into(),
            media_type: "text/plain".into(),
            width: None,
            height: None,
            byte_len: 12,
        }),
        TranscriptItem::Attachment(AttachmentMetadata {
            id: "clipboard".into(),
            path: None,
            display_name: "clipboard.png".into(),
            media_type: "image/png".into(),
            width: Some(1),
            height: Some(1),
            byte_len: 4,
        }),
    ];
    let snapshot = WorkspaceSnapshot::build(&state, 1_221.0, 992.0, Insets::ZERO);
    let rows = ThreadTranscript::visible_item_layout(
        snapshot.layout.transcript,
        &state.transcripts[&session],
    );

    for (index, label, path) in [
        (0, "文件：Report", "docs/report.md"),
        (1, "附件：notes.txt", "notes.txt"),
    ] {
        let node = snapshot
            .nodes
            .iter()
            .find(|node| node.name.starts_with(label))
            .expect(label);
        assert_eq!(node.rect, rows[index].visible_rect);
        assert_eq!(node.role, Role::Button);
        assert!(node.actions.contains(&Action::Click));
        assert_eq!(snapshot.hit_test(center(node.rect)), Some(node.id));
        assert_eq!(
            ThreadTranscript::command_for_widget(&state, node.id),
            Some(AppCommand::PreviewWorkspaceFile {
                session: session.clone(),
                relative_path: path.into(),
            })
        );
    }

    let clipboard = snapshot
        .nodes
        .iter()
        .find(|node| node.name.starts_with("附件：clipboard.png"))
        .expect("clipboard attachment");
    assert_eq!(clipboard.rect, rows[2].visible_rect);
    assert_eq!(clipboard.role, Role::Image);
    assert!(clipboard.actions.is_empty());
    assert_eq!(snapshot.hit_test(center(clipboard.rect)), None);
    assert_eq!(
        ThreadTranscript::command_for_widget(&state, clipboard.id),
        None
    );
}

#[test]
fn real_review_file_row_is_a_preview_button_with_one_shared_rect() {
    let (mut state, session, _) = state_with_session();
    state.presentation.secondary_pane = Some(SecondaryPane::Review);
    state.presentation.sessions.insert(
        session.clone(),
        SessionPresentationState {
            diff: SessionDiffState {
                dirty: false,
                load: LoadState::Ready(DiffSnapshot {
                    session: session.clone(),
                    files: vec![DiffFile {
                        path: "src/main.rs".into(),
                        status: DiffFileStatus::Modified,
                        additions: 2,
                        deletions: 1,
                    }],
                    unified: String::new(),
                }),
            },
            ..SessionPresentationState::default()
        },
    );
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let id = ReviewPanel::file_widget_id(&session, "src/main.rs");
    let node = snapshot.node(id).expect("review file row");
    let expected = ReviewPanel::file_row_layouts(snapshot.layout.review_panel, &state)[0].rect;

    assert_eq!(node.rect, expected);
    assert_eq!(node.role, Role::Button);
    assert!(node.actions.contains(&Action::Click));
    assert_eq!(snapshot.hit_test(center(node.rect)), Some(id));
    assert_eq!(
        ReviewPanel::command_for_widget(&state, id),
        Some(AppCommand::PreviewWorkspaceFile {
            session,
            relative_path: "src/main.rs".into(),
        })
    );
}

#[test]
fn document_preview_reuses_review_split_and_compact_primary_fallback() {
    let preview = WorkspaceLayout::compute_presentation(
        1_800.0,
        1_080.0,
        Insets::ZERO,
        ShellRoute::Conversation,
        Some(SecondaryPane::DocumentPreview),
    );
    let review = WorkspaceLayout::compute_presentation(
        1_800.0,
        1_080.0,
        Insets::ZERO,
        ShellRoute::Conversation,
        Some(SecondaryPane::Review),
    );
    let compact = WorkspaceLayout::compute_presentation(
        900.0,
        700.0,
        Insets::ZERO,
        ShellRoute::Conversation,
        Some(SecondaryPane::DocumentPreview),
    );

    assert_eq!(preview.review_panel, review.review_panel);
    assert_eq!(preview.review_panel.width(), 700.0);
    assert_eq!(preview.review_panel.min_x(), 1_100.0);
    assert_eq!(compact.review_panel.width(), 0.0);
    assert!(compact.primary_surface.width() > 0.0);
}

#[test]
fn ready_and_failed_preview_controls_are_accessible_and_emit_bound_commands() {
    let (mut state, session, workspace_uri) = state_with_session();
    let target = PreviewTarget {
        workspace_uri,
        relative_path: "docs/report.md".into(),
    };
    state.presentation.secondary_pane = Some(SecondaryPane::DocumentPreview);
    state
        .presentation
        .sessions
        .entry(session.clone())
        .or_default()
        .preview = PreviewState::Ready {
        target: target.clone(),
        title: "report.md".into(),
        content: "# Real report".into(),
        kind: PreviewKind::Markdown,
    };
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let layout = DocumentPreview::layout(snapshot.layout.review_panel, &state);
    let close = snapshot.node(layout.close_id).expect("preview close");
    let external = snapshot.node(layout.external_id).expect("external open");
    let title = snapshot
        .nodes
        .iter()
        .find(|node| node.name.contains("report.md"))
        .expect("preview title");

    assert_eq!(close.rect, layout.close_button);
    assert_eq!(
        external.rect,
        layout.external_button.expect("ready external")
    );
    assert_eq!(title.role, Role::Document);
    assert_eq!(
        DocumentPreview::command_for_widget(&state, external.id),
        Some(AppCommand::OpenPreviewExternally {
            session: session.clone(),
            relative_path: "docs/report.md".into(),
        })
    );

    state
        .presentation
        .sessions
        .get_mut(&session)
        .unwrap()
        .preview = PreviewState::Failed {
        target,
        message: "not found".into(),
    };
    let failed = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let failed_layout = DocumentPreview::layout(failed.layout.review_panel, &state);
    let retry = failed
        .node(failed_layout.retry_id)
        .expect("failed preview retry");
    assert_eq!(retry.rect, failed_layout.retry_button.expect("retry rect"));
    assert_eq!(
        DocumentPreview::command_for_widget(&state, retry.id),
        Some(AppCommand::PreviewWorkspaceFile {
            session,
            relative_path: "docs/report.md".into(),
        })
    );
}

#[test]
fn compact_preview_hard_wraps_long_markdown_and_plain_text_inside_content() {
    let (mut state, session, workspace_uri) = state_with_session();
    let target = PreviewTarget {
        workspace_uri,
        relative_path: "docs/compact.md".into(),
    };
    state.presentation.secondary_pane = Some(SecondaryPane::DocumentPreview);
    let rect = Rect::xywh(0.0, 0.0, 180.0, 280.0);

    for (content, kind) in [
        (
            "# 超长标题超长标题超长标题超长标题\n\n**abcdefghijklmnopqrstuvwxyz0123456789**",
            PreviewKind::Markdown,
        ),
        (
            "abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz",
            PreviewKind::PlainText,
        ),
    ] {
        state
            .presentation
            .sessions
            .entry(session.clone())
            .or_default()
            .preview = PreviewState::Ready {
            target: target.clone(),
            title: "compact.md".into(),
            content: content.into(),
            kind,
        };
        let layout = DocumentPreview::layout(rect, &state);
        let mut painter = PaintCapture::default();
        DocumentPreview::paint(&mut painter, rect, &state, &ZodeTheme::light());

        assert!(painter.clips.contains(&layout.content));
        let body = painter
            .texts
            .iter()
            .filter(|text| text.origin.y >= layout.content.origin.y)
            .collect::<Vec<_>>();
        assert!(body.len() > 1, "the long body must wrap");
        assert!(body.iter().all(|text| {
            text.origin.x >= layout.content.origin.x
                && text.origin.x + measured_width(&text.content, text.font_size)
                    <= layout.content.max_x() + 0.01
                && text.origin.y <= layout.content.max_y()
        }));
    }
}

#[test]
fn review_rows_require_available_local_workspace_and_clip_the_last_visible_row() {
    let (mut state, session, _) = state_with_session();
    state.presentation.sessions.insert(
        session.clone(),
        SessionPresentationState {
            diff: SessionDiffState {
                dirty: false,
                load: LoadState::Ready(DiffSnapshot {
                    session: session.clone(),
                    files: (0..4)
                        .map(|index| DiffFile {
                            path: format!("src/{index}.rs"),
                            status: DiffFileStatus::Modified,
                            additions: 1,
                            deletions: 0,
                        })
                        .collect(),
                    unified: String::new(),
                }),
            },
            ..SessionPresentationState::default()
        },
    );
    let panel = Rect::xywh(0.0, 0.0, 300.0, 101.0);
    let content = ReviewPanel::layout(panel).content;
    let rows = ReviewPanel::file_row_layouts(panel, &state);

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].rect.max_y(), content.max_y());
    assert!(rows[1].rect.size.y < 28.0);

    state
        .projects
        .iter_mut()
        .find(|project| Some(&project.workspace_uri) == state.active_workspace.as_ref())
        .unwrap()
        .available = false;
    assert!(ReviewPanel::file_row_layouts(panel, &state).is_empty());
}

#[test]
fn zero_sized_preview_exposes_no_empty_accessibility_nodes_and_caps_value() {
    let (mut state, session, workspace_uri) = state_with_session();
    state.presentation.secondary_pane = Some(SecondaryPane::DocumentPreview);
    state
        .presentation
        .sessions
        .entry(session)
        .or_default()
        .preview = PreviewState::Ready {
        target: PreviewTarget {
            workspace_uri,
            relative_path: "docs/large.md".into(),
        },
        title: "large.md".into(),
        content: "a".repeat(5_000),
        kind: PreviewKind::Markdown,
    };

    let empty = WorkspaceSnapshot::build(&state, 0.0, 0.0, Insets::ZERO);
    assert!(empty.node(DOCUMENT_PREVIEW_CLOSE_ID).is_none());
    assert!(empty.node(DOCUMENT_PREVIEW_CONTENT_ID).is_none());

    let visible = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let value = visible
        .node(DOCUMENT_PREVIEW_CONTENT_ID)
        .and_then(|node| node.value.as_deref())
        .expect("preview accessibility value");
    assert!(value.chars().count() < 2_100);
    assert!(value.ends_with('…'));
}

#[test]
fn retargeted_workspace_hides_stale_content_and_uses_a_contained_document_tab() {
    let (mut state, session, old_workspace) = state_with_session();
    state.presentation.secondary_pane = Some(SecondaryPane::DocumentPreview);
    state
        .presentation
        .sessions
        .entry(session.clone())
        .or_default()
        .preview = PreviewState::Ready {
        target: PreviewTarget {
            workspace_uri: old_workspace,
            relative_path: "docs/old.md".into(),
        },
        title: "old.md".into(),
        content: "stale secret".into(),
        kind: PreviewKind::Markdown,
    };
    assert_eq!(DocumentPreview::tab_label(&state), "old.md");

    let new_workspace = WorkspaceUri::new("file:///repo/new").unwrap();
    state.projects.push(ProjectState {
        workspace_uri: new_workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
    state
        .threads
        .iter_mut()
        .find(|thread| thread.session == session)
        .unwrap()
        .workspace_uri = new_workspace.clone();
    state.active_workspace = Some(new_workspace);

    let rect = Rect::xywh(0.0, 0.0, 360.0, 420.0);
    let layout = DocumentPreview::layout(rect, &state);
    let mut painter = PaintCapture::default();
    DocumentPreview::paint(&mut painter, rect, &state, &ZodeTheme::light());

    assert_eq!(DocumentPreview::tab_label(&state), "文档预览");
    assert!(layout.tab.origin.x >= layout.header.origin.x);
    assert!(layout.tab.max_x() <= layout.close_button.origin.x);
    assert!(painter.texts.iter().any(|text| text.content == "文档预览"));
    assert!(!painter
        .texts
        .iter()
        .any(|text| text.content.contains("stale secret") || text.content == "old.md"));
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    assert!(!snapshot.nodes.iter().any(|node| {
        node.name.contains("old.md")
            || node
                .value
                .as_deref()
                .is_some_and(|value| value.contains("stale secret"))
    }));
}

#[test]
fn selected_document_tab_tracks_ready_loading_and_failed_targets_without_overlap() {
    let (mut state, session, workspace_uri) = state_with_session();
    state.presentation.secondary_pane = Some(SecondaryPane::DocumentPreview);
    let target = PreviewTarget {
        workspace_uri,
        relative_path: "docs/report.md".into(),
    };
    let rect = Rect::xywh(0.0, 0.0, 260.0, 420.0);

    for preview in [
        PreviewState::Ready {
            target: target.clone(),
            title: "report.md".into(),
            content: "ready".into(),
            kind: PreviewKind::Markdown,
        },
        PreviewState::Loading {
            target: target.clone(),
        },
        PreviewState::Failed {
            target: target.clone(),
            message: "not found".into(),
        },
    ] {
        state
            .presentation
            .sessions
            .entry(session.clone())
            .or_default()
            .preview = preview;
        let layout = DocumentPreview::layout(rect, &state);
        let controls_left = layout
            .retry_button
            .or(layout.external_button)
            .map_or(layout.close_button.origin.x, |button| button.origin.x);
        assert_eq!(DocumentPreview::tab_label(&state), "report.md");
        assert!(layout.tab.origin.x >= layout.header.origin.x);
        assert!(layout.tab.max_x() <= controls_left);
        assert!(layout.tab.max_y() <= layout.header.max_y());

        let mut painter = PaintCapture::default();
        DocumentPreview::paint(&mut painter, rect, &state, &ZodeTheme::light());
        assert!(painter.texts.iter().any(|text| text.content == "report.md"));
        assert!(!painter.texts.iter().any(|text| text.content == "预览"));
    }
}

#[test]
fn document_preview_controls_use_centered_semantic_icons() {
    let (mut state, session, workspace_uri) = state_with_session();
    state.presentation.secondary_pane = Some(SecondaryPane::DocumentPreview);
    let target = PreviewTarget {
        workspace_uri,
        relative_path: "docs/report.md".into(),
    };
    state
        .presentation
        .sessions
        .entry(session.clone())
        .or_default()
        .preview = PreviewState::Ready {
        target: target.clone(),
        title: "report.md".into(),
        content: "ready".into(),
        kind: PreviewKind::Markdown,
    };
    let rect = Rect::xywh(1_100.0, 0.0, 700.0, 1_080.0);
    let layout = DocumentPreview::layout(rect, &state);
    let mut ready = PaintCapture::default();

    DocumentPreview::paint(&mut ready, rect, &state, &ZodeTheme::light());

    for (icon, bounds) in [
        (SemanticIcon::FileText, layout.tab),
        (SemanticIcon::Close, layout.close_button),
        (
            SemanticIcon::ExternalOpen,
            layout.external_button.expect("external button"),
        ),
    ] {
        let svg = ready.svg(icon);
        assert_close(
            svg.top_left.y + svg.size / 2.0,
            bounds.origin.y + bounds.size.y / 2.0,
        );
    }
    let close = ready.svg(SemanticIcon::Close);
    assert_close(
        close.top_left.x + close.size / 2.0,
        layout.close_button.origin.x + layout.close_button.size.x / 2.0,
    );
    assert!(!ready
        .texts
        .iter()
        .any(|text| text.content == "▣" || text.content == "×"));

    state
        .presentation
        .sessions
        .get_mut(&session)
        .expect("session presentation")
        .preview = PreviewState::Failed {
        target,
        message: "offline".into(),
    };
    let failed_layout = DocumentPreview::layout(rect, &state);
    let mut failed = PaintCapture::default();
    DocumentPreview::paint(&mut failed, rect, &state, &ZodeTheme::light());
    let refresh = failed.svg(SemanticIcon::Refresh);
    let retry = failed_layout.retry_button.expect("retry button");
    assert_close(
        refresh.top_left.y + refresh.size / 2.0,
        retry.origin.y + retry.size.y / 2.0,
    );
}

#[derive(Default)]
struct PaintCapture {
    clips: Vec<Rect>,
    texts: Vec<PaintedText>,
    svgs: Vec<PaintedSvg>,
}

impl PaintCapture {
    fn svg(&self, icon: SemanticIcon) -> &PaintedSvg {
        self.svgs
            .iter()
            .find(|svg| svg.path == icon.path())
            .unwrap_or_else(|| panic!("missing semantic icon: {icon:?}"))
    }
}

struct PaintedText {
    content: String,
    origin: Point2D,
    font_size: f32,
}

struct PaintedSvg {
    path: String,
    top_left: Point2D,
    size: f32,
}

impl Painter for PaintCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, origin: Point2D) {
        self.texts.push(PaintedText {
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
    fn clip_rect(&mut self, rect: Rect) {
        self.clips.push(rect);
    }
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color) {}
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
    fn stroke_svg_path(
        &mut self,
        d: &str,
        top_left: Point2D,
        size: f32,
        _color: Color,
        _width: f32,
    ) {
        self.svgs.push(PaintedSvg {
            path: d.to_owned(),
            top_left,
            size,
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

fn measured_width(text: &str, font_size: f32) -> f32 {
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

fn center(rect: jian_widgets::Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 0.01,
        "expected {actual} to be aligned with {expected}"
    );
}
