use std::sync::Mutex;

use zode_app_model::{
    demo_state, AppCommand, AttachmentMetadata, ComingSoonFeature, EnvironmentActionKind,
    EnvironmentSnapshot, IntegrationsTab, LoadState, PreviewKind, PreviewState, PreviewTarget,
    ProjectState, SecondaryPane, SettingsCategory, SettingsCommandOutcome, ShellRoute,
    TranscriptState,
};
use zode_app_ui::{
    ComposerController, ComposerOutcome, ComposerSubmission, Insets, Key, Modifiers, ProjectPicker,
    SettingsPanel, ThreadHeader, WidgetId, WorkspaceSnapshot, ENVIRONMENT_CLOSE_ID,
    HEADER_ENVIRONMENT_ID, PROJECT_DETACH_ID, PROJECT_PICKER_NEW_ID, PROJECT_PICKER_PROJECTLESS_ID,
    PROJECT_PICKER_TRIGGER_ID,
};
use zode_node_protocol::{
    DiffFile, DiffFileStatus, DiffSnapshot, SessionLocator, ThreadStatus, ThreadSummary,
    UserContent, WorkspaceUri,
};

use super::super::session_menu::{
    session_menu_escape_command as close_session_menu_command, session_menu_outside_click_command,
};
use super::{
    consume_external_preview_command, normalize_conversation_route, project_composer_outcome,
    reduce_local_settings_command, settings_interaction_viewport, widget_command,
    widget_command_for_snapshot,
};
use crate::services::{ExternalOpenService, ServiceError};
use crate::{command_bridge::prepare_dispatch, event_map::composer_outcome_command};

#[derive(Default)]
struct FakeExternalOpen {
    calls: Mutex<Vec<(WorkspaceUri, String)>>,
    fail: bool,
}

#[test]
fn empty_task_project_controls_map_to_task_context_commands() {
    let (mut state, _, workspace_uri) = state_with_session();
    state.current_session = None;

    assert_eq!(
        widget_command(&state, PROJECT_PICKER_TRIGGER_ID),
        Some(AppCommand::ToggleProjectPicker)
    );
    assert_eq!(
        widget_command(&state, PROJECT_DETACH_ID),
        Some(AppCommand::BeginTask {
            workspace_uri: None,
        })
    );

    state.project_picker.open = true;
    assert_eq!(
        widget_command(&state, ProjectPicker::project_widget_id(&workspace_uri)),
        Some(AppCommand::BeginTask {
            workspace_uri: Some(workspace_uri),
        })
    );
    assert_eq!(
        widget_command(&state, PROJECT_PICKER_NEW_ID),
        Some(AppCommand::CreateProject)
    );
    assert_eq!(
        widget_command(&state, PROJECT_PICKER_PROJECTLESS_ID),
        Some(AppCommand::BeginTask {
            workspace_uri: None,
        })
    );
}

#[test]
fn pinned_summary_toggle_is_responsive_to_the_current_snapshot() {
    let (mut state, _, _) = state_with_session();
    let wide = WorkspaceSnapshot::build(&state, 1_800.0, 900.0, Insets::ZERO);
    assert_eq!(
        widget_command_for_snapshot(&state, &wide, HEADER_ENVIRONMENT_ID),
        Some(AppCommand::SetPinnedSummaryAutoHidden(true))
    );
    assert_eq!(
        widget_command_for_snapshot(&state, &wide, ENVIRONMENT_CLOSE_ID),
        Some(AppCommand::SetPinnedSummaryAutoHidden(true))
    );

    state.presentation.pinned_summary_auto_hidden = true;
    let dismissed = WorkspaceSnapshot::build(&state, 1_800.0, 900.0, Insets::ZERO);
    assert_eq!(
        widget_command_for_snapshot(&state, &dismissed, HEADER_ENVIRONMENT_ID),
        Some(AppCommand::SetPinnedSummaryAutoHidden(false))
    );

    state.presentation.pinned_summary_auto_hidden = false;
    let narrow = WorkspaceSnapshot::build(&state, 1_200.0, 900.0, Insets::ZERO);
    assert_eq!(
        widget_command_for_snapshot(&state, &narrow, HEADER_ENVIRONMENT_ID),
        Some(AppCommand::OpenSecondary(SecondaryPane::Environment))
    );

    state.presentation.secondary_pane = Some(SecondaryPane::Environment);
    let overlay = WorkspaceSnapshot::build(&state, 1_200.0, 900.0, Insets::ZERO);
    assert_eq!(
        widget_command_for_snapshot(&state, &overlay, HEADER_ENVIRONMENT_ID),
        Some(AppCommand::CloseSecondary)
    );
    assert_eq!(
        widget_command_for_snapshot(&state, &overlay, ENVIRONMENT_CLOSE_ID),
        Some(AppCommand::CloseSecondary)
    );
}

impl ExternalOpenService for FakeExternalOpen {
    fn open_file(&self, workspace: &WorkspaceUri, relative: &str) -> Result<(), ServiceError> {
        self.calls
            .lock()
            .unwrap()
            .push((workspace.clone(), relative.into()));
        if self.fail {
            Err(ServiceError::Platform("external open failed".into()))
        } else {
            Ok(())
        }
    }

    fn open_url(&self, _url: &str) -> Result<(), ServiceError> {
        Err(ServiceError::Platform("unexpected url".into()))
    }
}

fn state_with_session() -> (zode_app_model::ZodeAppState, SessionLocator, WorkspaceUri) {
    let mut state = demo_state();
    let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
    let session = SessionLocator::new(state.host.node_id, "session");
    state.projects.push(ProjectState {
        workspace_uri: workspace_uri.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace_uri.clone(),
        title: "session".into(),
        updated_at_ms: 0,
        status: ThreadStatus::Idle,
    });
    state
        .transcripts
        .insert(session.clone(), TranscriptState::default());
    state.current_session = Some(session.clone());
    state.active_workspace = Some(workspace_uri.clone());
    (state, session, workspace_uri)
}

#[test]
fn static_sidebar_ids_map_to_typed_commands() {
    let (state, _, workspace_uri) = state_with_session();
    let expected = [
        (
            2,
            AppCommand::BeginTask {
                workspace_uri: Some(workspace_uri.clone()),
            },
        ),
        (
            3,
            AppCommand::Navigate(ShellRoute::ComingSoon(ComingSoonFeature::ScheduledTasks)),
        ),
        (
            4,
            AppCommand::Navigate(ShellRoute::Integrations(IntegrationsTab::Plugins)),
        ),
        (
            5,
            AppCommand::Navigate(ShellRoute::ComingSoon(ComingSoonFeature::Sites)),
        ),
        (
            6,
            AppCommand::Navigate(ShellRoute::ComingSoon(ComingSoonFeature::PullRequests)),
        ),
        (
            7,
            AppCommand::Navigate(ShellRoute::ComingSoon(ComingSoonFeature::Chats)),
        ),
        (
            9,
            AppCommand::Navigate(ShellRoute::Settings(SettingsCategory::General)),
        ),
        (
            10,
            AppCommand::Navigate(ShellRoute::ComingSoon(ComingSoonFeature::Help)),
        ),
    ];

    for (id, command) in expected {
        assert_eq!(widget_command(&state, WidgetId(id)), Some(command));
    }
}

#[test]
fn page_and_pane_widget_ids_map_through_component_commands() {
    let (mut state, session, _) = state_with_session();
    let presentation = state
        .presentation
        .sessions
        .entry(session.clone())
        .or_default();
    presentation.diff.load = LoadState::Ready(DiffSnapshot {
        session: session.clone(),
        files: vec![DiffFile {
            path: "src/main.rs".into(),
            status: DiffFileStatus::Modified,
            additions: 1,
            deletions: 0,
        }],
        unified: String::new(),
    });
    presentation.context = LoadState::Ready(EnvironmentSnapshot {
        workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
        branch: Some("codex/test".into()),
        subagents: Vec::new(),
        background_processes: Vec::new(),
        sources: Vec::new(),
    });
    let expected = [
        (60, AppCommand::OpenSecondary(SecondaryPane::Environment)),
        (61, AppCommand::OpenReview),
        (
            70,
            AppCommand::SelectIntegrationsTab(IntegrationsTab::Plugins),
        ),
        (
            71,
            AppCommand::SelectIntegrationsTab(IntegrationsTab::Skills),
        ),
        (
            80,
            AppCommand::SelectSettingsCategory(SettingsCategory::General),
        ),
        (
            81,
            AppCommand::SelectSettingsCategory(SettingsCategory::Appearance),
        ),
        (
            8_109,
            AppCommand::Navigate(ShellRoute::Integrations(IntegrationsTab::Plugins)),
        ),
        (100, AppCommand::CloseSecondary),
        (
            101,
            AppCommand::RunEnvironmentAction {
                session: session.clone(),
                action: EnvironmentActionKind::CompareWorkspaceToHead,
            },
        ),
        (102, AppCommand::CloseSecondary),
        (
            200,
            AppCommand::RunEnvironmentAction {
                session: session.clone(),
                action: EnvironmentActionKind::RefreshStatus,
            },
        ),
        (
            201,
            AppCommand::RunEnvironmentAction {
                session: session.clone(),
                action: EnvironmentActionKind::OpenWorkspace,
            },
        ),
    ];

    for (id, command) in expected {
        assert_eq!(widget_command(&state, WidgetId(id)), Some(command));
    }

    state
        .presentation
        .sessions
        .get_mut(&session)
        .unwrap()
        .preview = PreviewState::Failed {
        target: PreviewTarget {
            workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
            relative_path: "docs/report.md".into(),
        },
        message: "retry".into(),
    };
    state.presentation.secondary_pane = Some(SecondaryPane::DocumentPreview);
    assert_eq!(
        widget_command(&state, WidgetId(103)),
        Some(AppCommand::CloseSecondary)
    );
    assert_eq!(
        widget_command(&state, WidgetId(104)),
        Some(AppCommand::OpenPreviewExternally {
            session: session.clone(),
            relative_path: "docs/report.md".into(),
        })
    );
    assert_eq!(
        widget_command(&state, WidgetId(105)),
        Some(AppCommand::PreviewWorkspaceFile {
            session,
            relative_path: "docs/report.md".into(),
        })
    );
}

#[test]
fn task_menu_closes_on_escape_or_outside_click_but_not_inside_click() {
    let (mut state, session, _) = state_with_session();
    state.session_menu = Some(session.clone());
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let menu = ThreadHeader::menu_layout(snapshot.layout.top_bar, &state).unwrap();
    let close = AppCommand::ToggleSessionMenu {
        session: session.clone(),
    };

    assert_eq!(close_session_menu_command(&state), Some(close.clone()));
    let inside = jian_widgets::Point2D::new(
        menu.pin.rect.origin.x + menu.pin.rect.size.x / 2.0,
        menu.pin.rect.origin.y + menu.pin.rect.size.y / 2.0,
    );
    assert_eq!(
        session_menu_outside_click_command(&state, &snapshot, inside),
        None
    );
    assert_eq!(
        session_menu_outside_click_command(
            &state,
            &snapshot,
            jian_widgets::Point2D::new(
                snapshot.layout.composer.origin.x,
                snapshot.layout.composer.origin.y,
            ),
        ),
        Some(close)
    );

    state.current_session = Some(SessionLocator::new(state.host.node_id, "other"));
    assert_eq!(close_session_menu_command(&state), None);
}

#[test]
fn nested_copy_and_rename_surfaces_keep_inside_clicks_and_close_outside() {
    let (mut state, session, _) = state_with_session();
    state.session_menu = Some(session.clone());
    state.session_copy_menu = Some(session.clone());
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let copy = ThreadHeader::menu_layout(snapshot.layout.top_bar, &state)
        .unwrap()
        .copy_menu
        .unwrap();
    let inside_copy = jian_widgets::Point2D::new(
        copy.title.rect.origin.x + copy.title.rect.size.x / 2.0,
        copy.title.rect.origin.y + copy.title.rect.size.y / 2.0,
    );
    assert_eq!(
        session_menu_outside_click_command(&state, &snapshot, inside_copy),
        None
    );
    assert_eq!(
        session_menu_outside_click_command(
            &state,
            &snapshot,
            jian_widgets::Point2D::new(
                snapshot.layout.composer.origin.x,
                snapshot.layout.composer.origin.y,
            ),
        ),
        Some(AppCommand::ToggleSessionMenu {
            session: session.clone()
        })
    );

    state.close_session_action_surfaces();
    state.session_rename = Some(zode_app_model::SessionRenameState {
        session: session.clone(),
        draft: "renamed".into(),
    });
    let rename_snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let rename = ThreadHeader::rename_layout(rename_snapshot.layout.top_bar, &state).unwrap();
    assert_eq!(
        session_menu_outside_click_command(
            &state,
            &rename_snapshot,
            jian_widgets::Point2D::new(rename.input.origin.x + 4.0, rename.input.origin.y + 4.0),
        ),
        None
    );
    assert_eq!(
        session_menu_outside_click_command(
            &state,
            &rename_snapshot,
            jian_widgets::Point2D::new(
                rename_snapshot.layout.composer.origin.x,
                rename_snapshot.layout.composer.origin.y,
            ),
        ),
        Some(AppCommand::CancelRenameSession { session })
    );
}

#[test]
fn permission_revoke_widget_keeps_its_endpoint_command() {
    let (mut state, _, workspace_uri) = state_with_session();
    state.project_permissions.insert(
        workspace_uri.clone(),
        LoadState::Ready(vec!["write_file".into()]),
    );
    let id = SettingsPanel::permission_widget_id(&workspace_uri, "write_file");

    assert_eq!(
        widget_command(&state, id),
        Some(AppCommand::RevokeProjectPermission {
            workspace_uri,
            tool: "write_file".into(),
        })
    );
}

#[test]
fn permission_revoke_is_not_consumed_as_a_local_settings_update() {
    let (mut state, _, workspace_uri) = state_with_session();
    state.project_permissions.insert(
        workspace_uri.clone(),
        LoadState::Ready(vec!["write_file".into()]),
    );
    let command = AppCommand::RevokeProjectPermission {
        workspace_uri: workspace_uri.clone(),
        tool: "write_file".into(),
    };

    assert_eq!(
        reduce_local_settings_command(&mut state, command.clone()),
        SettingsCommandOutcome::Ignored
    );
    assert_eq!(
        state.project_permissions[&workspace_uri],
        LoadState::Ready(vec!["write_file".into()])
    );
    assert!(crate::command_bridge::prepare_dispatch(&mut state, command)
        .unwrap()
        .is_some());
}

#[test]
fn settings_input_uses_the_same_page_viewport_as_paint_and_accessibility() {
    let mut state = demo_state();
    state.presentation.route = ShellRoute::Settings(SettingsCategory::General);
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let expected = SettingsPanel::page_layout(snapshot.layout.primary_surface).0;

    assert_eq!(settings_interaction_viewport(&snapshot), expected);
    assert_ne!(
        settings_interaction_viewport(&snapshot),
        snapshot.layout.transcript
    );
}

#[test]
fn session_and_new_task_commands_normalize_to_conversation() {
    let (mut state, session, workspace_uri) = state_with_session();
    state.presentation.route = ShellRoute::Integrations(IntegrationsTab::Skills);
    state.presentation.secondary_pane = Some(SecondaryPane::Review);
    state.review.open = true;

    normalize_conversation_route(&mut state, &AppCommand::SelectSession(session));
    assert_eq!(state.presentation.route, ShellRoute::Conversation);
    assert_eq!(state.presentation.secondary_pane, None);
    assert!(!state.review.open);

    state.presentation.route = ShellRoute::ComingSoon(ComingSoonFeature::Sites);
    normalize_conversation_route(
        &mut state,
        &AppCommand::BeginTask {
            workspace_uri: Some(workspace_uri),
        },
    );
    assert_eq!(state.presentation.route, ShellRoute::Conversation);
}

#[test]
fn interaction_source_has_no_direct_settings_page_mutation() {
    let source = include_str!("interaction.rs");
    assert!(!source.contains("self.app_state.shell.page = ShellPage::Settings"));
    assert!(!source.contains("self.app_state.shell.page = ShellPage::Conversation"));
    assert!(source.contains("AppCommand::Navigate(ShellRoute::Conversation)"));
    let activation = source
        .split("pub(super) fn activate_widget")
        .nth(1)
        .and_then(|tail| {
            tail.split("pub(super) fn drain_accessibility_actions")
                .next()
        })
        .expect("activate_widget source is present");
    assert!(activation
        .contains("widget_command_for_snapshot(&self.app_state, &self.frame_snapshot, id)"));
    assert!(activation.contains("self.enqueue_command(command)"));
}

#[test]
fn interaction_page_behavior_reads_the_typed_route() {
    let source = include_str!("interaction.rs");
    assert!(!source.contains("app_state.shell.page"));
    assert!(source.contains("app_state.presentation.route"));
}

#[test]
fn applying_composer_outcome_projects_attachment_metadata() {
    let mut state = demo_state();
    let metadata = AttachmentMetadata {
        id: "attachment-1".into(),
        path: None,
        display_name: "shot.png".into(),
        media_type: "image/png".into(),
        width: Some(640),
        height: Some(360),
        byte_len: 1_024,
    };

    project_composer_outcome(
        &mut state,
        &ComposerOutcome::AttachmentsChanged(vec![metadata.clone()]),
    );

    assert_eq!(state.composer.attachments, vec![metadata]);
}

#[test]
fn busy_composer_queues_without_emitting_a_steer_command() {
    let mut composer = ComposerController::new("排到当前任务后面");
    composer.set_busy(true);

    let mut outcome = composer.key(Key::Enter, Modifiers::NONE);

    let ComposerOutcome::Queue(submission) = &outcome else {
        panic!("busy Enter must produce an explicit queue outcome, got {outcome:?}");
    };
    assert!(matches!(
        submission.content.as_slice(),
        [UserContent::Text { text }] if text == "排到当前任务后面"
    ));
    assert_eq!(composer_outcome_command(&mut outcome), None);
}

#[test]
fn queue_edit_restores_the_original_draft_and_attachment_payload() {
    let mut composer = ComposerController::new("尚未发送的原草稿");
    let added = composer.paste_image("image/png", "cHJpdmF0ZS1pbWFnZQ==", "reference.png");
    let ComposerOutcome::AttachmentsChanged(original_metadata) = added else {
        panic!("image paste must project attachment metadata");
    };

    assert!(composer.begin_queue_edit("队列里的旧文字"));
    assert_eq!(composer.text(), "队列里的旧文字");
    assert!(composer.attachment_metadata().is_empty());
    composer.set_text("队列里的新文字");
    assert_eq!(
        composer.paste_image("image/png", "bmV3LWltYWdl", "ignored.png"),
        ComposerOutcome::Ignored,
        "queue editing is text-only so it cannot replace the private payload"
    );

    assert!(composer.finish_queue_edit());
    assert_eq!(composer.text(), "尚未发送的原草稿");
    assert_eq!(composer.attachment_metadata(), original_metadata);

    let ComposerOutcome::Send(restored) = composer.key(Key::Enter, Modifiers::NONE) else {
        panic!("restored draft should remain independently sendable");
    };
    assert!(matches!(
        restored.content.first(),
        Some(UserContent::Text { text }) if text == "尚未发送的原草稿"
    ));
    assert!(matches!(
        restored.content.get(1),
        Some(UserContent::Image { data_base64, display_name, .. })
            if data_base64 == "cHJpdmF0ZS1pbWFnZQ==" && display_name == "reference.png"
    ));
}

#[test]
fn sending_clears_projected_attachments_without_dropping_payload() {
    let mut state = demo_state();
    state.composer.attachments.push(AttachmentMetadata {
        id: "attachment-1".into(),
        path: None,
        display_name: "shot.png".into(),
        media_type: "image/png".into(),
        width: Some(640),
        height: Some(360),
        byte_len: 1_024,
    });
    let outcome = ComposerOutcome::Send(ComposerSubmission {
        content: vec![UserContent::Image {
            mime_type: "image/png".into(),
            data_base64: "cGF5bG9hZA==".into(),
            display_name: "shot.png".into(),
        }],
        attachments: state.composer.attachments.clone(),
    });

    project_composer_outcome(&mut state, &outcome);

    assert!(state.composer.attachments.is_empty());
    let ComposerOutcome::Send(submission) = outcome else {
        unreachable!();
    };
    assert!(matches!(
        &submission.content[0],
        UserContent::Image { data_base64, .. } if data_base64 == "cGF5bG9hZA=="
    ));
}

#[test]
fn composer_attachment_metadata_enters_the_transcript_after_sending() {
    let (mut state, session, _) = state_with_session();
    let attachment = AttachmentMetadata {
        id: "attachment-1".into(),
        path: None,
        display_name: "shot.png".into(),
        media_type: "image/png".into(),
        width: Some(640),
        height: Some(360),
        byte_len: 1_024,
    };
    let outcome = ComposerOutcome::Send(ComposerSubmission {
        content: vec![UserContent::Image {
            mime_type: "image/png".into(),
            data_base64: "cGF5bG9hZA==".into(),
            display_name: "shot.png".into(),
        }],
        attachments: vec![attachment.clone()],
    });

    project_composer_outcome(&mut state, &outcome);

    assert!(matches!(
        state.transcripts[&session].items.last(),
        Some(zode_app_model::TranscriptItem::Attachment(projected)) if projected == &attachment
    ));
}

#[test]
fn first_attachment_submit_creates_session_before_metadata_projection() {
    let mut state = demo_state();
    let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
    state.projects.push(ProjectState {
        workspace_uri: workspace_uri.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
    state.active_workspace = Some(workspace_uri);
    let attachment = AttachmentMetadata {
        id: "attachment-first".into(),
        path: None,
        display_name: "first.png".into(),
        media_type: "image/png".into(),
        width: Some(640),
        height: Some(360),
        byte_len: 1_024,
    };
    let mut outcome = ComposerOutcome::Send(ComposerSubmission {
        content: vec![UserContent::Image {
            mime_type: "image/png".into(),
            data_base64: "cGF5bG9hZA==".into(),
            display_name: "first.png".into(),
        }],
        attachments: vec![attachment.clone()],
    });

    let command = composer_outcome_command(&mut outcome).expect("submit command");
    let ComposerOutcome::Send(submission) = &outcome else {
        unreachable!();
    };
    assert!(submission.content.is_empty());
    assert_eq!(
        submission.attachments.as_slice(),
        std::slice::from_ref(&attachment)
    );

    let dispatch = prepare_dispatch(&mut state, command)
        .expect("first submit is valid")
        .expect("first submit dispatch");
    assert!(format!("{dispatch:?}").contains("CreateSession"));
    assert!(format!("{dispatch:?}").contains("StartTurn"));
    let session = state.current_session.clone().expect("created session");
    assert!(state.transcripts.contains_key(&session));

    project_composer_outcome(&mut state, &outcome);

    assert!(matches!(
        state.transcripts[&session].items.last(),
        Some(zode_app_model::TranscriptItem::Attachment(projected)) if projected == &attachment
    ));
}

#[test]
fn external_preview_open_is_session_validated_locally_and_projects_failure() {
    let (mut state, session, workspace_uri) = state_with_session();
    let target = PreviewTarget {
        workspace_uri: workspace_uri.clone(),
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
        content: "# report".into(),
        kind: PreviewKind::Markdown,
    };
    let external = FakeExternalOpen::default();

    assert!(consume_external_preview_command(
        &mut state,
        &external,
        &AppCommand::OpenPreviewExternally {
            session: session.clone(),
            relative_path: "docs/report.md".into(),
        },
    ));
    assert_eq!(
        external.calls.lock().unwrap().as_slice(),
        &[(workspace_uri.clone(), "docs/report.md".into())]
    );

    let other = SessionLocator::new(state.host.node_id, "other");
    assert!(consume_external_preview_command(
        &mut state,
        &external,
        &AppCommand::OpenPreviewExternally {
            session: other,
            relative_path: "docs/secret.md".into(),
        },
    ));
    assert_eq!(external.calls.lock().unwrap().len(), 1);

    let failing = FakeExternalOpen {
        fail: true,
        ..FakeExternalOpen::default()
    };
    assert!(consume_external_preview_command(
        &mut state,
        &failing,
        &AppCommand::OpenPreviewExternally {
            session: session.clone(),
            relative_path: "docs/report.md".into(),
        },
    ));
    assert!(matches!(
        &state.presentation.sessions[&session].preview,
        PreviewState::Failed { target: failed_target, message }
            if failed_target == &target && message.contains("external open failed")
    ));
}
