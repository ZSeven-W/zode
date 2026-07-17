mod snapshot_support;

use std::path::{Path, PathBuf};

use snapshot_support::{
    assert_case_geometry, assert_platform_snapshot, compare_reference_images, named_scene,
    reference_scenes, render_snapshot, scene_names, GeometryExpectation, LayoutRect,
    ReferenceScene, SnapshotCase, REFERENCE_SCENE_NAMES,
};
use zode_app_model::{
    environment_sections, LoadState, PreviewState, SecondaryPane, ThemePreference,
};
use zode_app_ui::{
    Composer, Insets, RectExt, SettingsPanel, WorkspaceSnapshot, TRANSCRIPT_COMPOSER_GAP,
};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

const WIDTH: u32 = 1800;
const HEIGHT: u32 = 1080;
const SCALE: f32 = 1.0;

const EMPTY_TASK_GEOMETRY: &[GeometryExpectation] = &[
    GeometryExpectation::new(LayoutRect::Viewport, 0.0, 0.0, 1800.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Sidebar, 0.0, 0.0, 240.0, 1080.0),
    GeometryExpectation::new(LayoutRect::TopBar, 240.0, 0.0, 1560.0, 46.0),
    GeometryExpectation::new(LayoutRect::PrimarySurface, 240.0, 0.0, 1560.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Transcript, 652.0, 70.0, 736.0, 830.0),
    GeometryExpectation::new(LayoutRect::Composer, 652.0, 928.0, 736.0, 138.0),
];

const FULL_PAGE_GEOMETRY: &[GeometryExpectation] = &[
    GeometryExpectation::new(LayoutRect::Viewport, 0.0, 0.0, 1800.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Sidebar, 0.0, 0.0, 240.0, 1080.0),
    GeometryExpectation::new(LayoutRect::TopBar, 240.0, 0.0, 1560.0, 46.0),
    GeometryExpectation::new(LayoutRect::PrimarySurface, 240.0, 0.0, 1560.0, 1080.0),
    GeometryExpectation::new(LayoutRect::PageContent, 652.0, 70.0, 736.0, 1010.0),
];

const SETTINGS_GEOMETRY: &[GeometryExpectation] = &[
    GeometryExpectation::new(LayoutRect::Viewport, 0.0, 0.0, 1800.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Sidebar, 0.0, 0.0, 240.0, 1080.0),
    GeometryExpectation::new(LayoutRect::TopBar, 240.0, 0.0, 1560.0, 46.0),
    GeometryExpectation::new(LayoutRect::PrimarySurface, 240.0, 0.0, 1560.0, 1080.0),
    GeometryExpectation::new(LayoutRect::PageContent, 636.0, 70.0, 768.0, 1010.0),
];

const DOCUMENT_PREVIEW_GEOMETRY: &[GeometryExpectation] = &[
    GeometryExpectation::new(LayoutRect::Viewport, 0.0, 0.0, 1800.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Sidebar, 0.0, 0.0, 240.0, 1080.0),
    GeometryExpectation::new(LayoutRect::TopBar, 240.0, 0.0, 929.0, 46.0),
    GeometryExpectation::new(LayoutRect::PrimarySurface, 240.0, 0.0, 929.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Transcript, 336.5, 70.0, 736.0, 830.0),
    GeometryExpectation::new(LayoutRect::Composer, 336.5, 928.0, 736.0, 138.0),
    GeometryExpectation::new(LayoutRect::Divider, 1169.0, 0.0, 1.0, 1080.0),
    GeometryExpectation::new(LayoutRect::ReviewPanel, 1170.0, 0.0, 630.0, 1080.0),
];

const ARTIFACTS_GEOMETRY: &[GeometryExpectation] = &[
    GeometryExpectation::new(LayoutRect::Viewport, 0.0, 0.0, 1800.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Sidebar, 0.0, 0.0, 240.0, 1080.0),
    GeometryExpectation::new(LayoutRect::TopBar, 240.0, 0.0, 1560.0, 46.0),
    GeometryExpectation::new(LayoutRect::PrimarySurface, 240.0, 0.0, 1560.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Transcript, 652.0, 70.0, 736.0, 778.0),
    GeometryExpectation::new(LayoutRect::Composer, 652.0, 876.0, 736.0, 190.0),
    GeometryExpectation::new(LayoutRect::ContextPanel, 1484.0, 62.0, 300.0, 1002.0),
];

const ENVIRONMENT_GEOMETRY: &[GeometryExpectation] = &[
    GeometryExpectation::new(LayoutRect::Viewport, 0.0, 0.0, 1800.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Sidebar, 0.0, 0.0, 240.0, 1080.0),
    GeometryExpectation::new(LayoutRect::TopBar, 240.0, 0.0, 1560.0, 46.0),
    GeometryExpectation::new(LayoutRect::PrimarySurface, 240.0, 0.0, 1560.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Transcript, 652.0, 70.0, 736.0, 830.0),
    GeometryExpectation::new(LayoutRect::Composer, 652.0, 928.0, 736.0, 138.0),
    GeometryExpectation::new(LayoutRect::ContextPanel, 1484.0, 62.0, 300.0, 1002.0),
];

const QUEUE_GEOMETRY: &[GeometryExpectation] = &[
    GeometryExpectation::new(LayoutRect::Viewport, 0.0, 0.0, 1800.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Sidebar, 0.0, 0.0, 240.0, 1080.0),
    GeometryExpectation::new(LayoutRect::TopBar, 240.0, 0.0, 1560.0, 46.0),
    GeometryExpectation::new(LayoutRect::PrimarySurface, 240.0, 0.0, 1560.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Transcript, 494.0, 70.0, 736.0, 704.0),
    GeometryExpectation::new(LayoutRect::Composer, 494.0, 802.0, 736.0, 264.0),
    GeometryExpectation::new(LayoutRect::ContextPanel, 1484.0, 62.0, 300.0, 1002.0),
];

fn case_for(name: &'static str) -> SnapshotCase {
    let geometry = match name {
        "empty-task" => EMPTY_TASK_GEOMETRY,
        "integrations-catalog" => FULL_PAGE_GEOMETRY,
        "settings-general" => SETTINGS_GEOMETRY,
        "conversation-document-preview" => DOCUMENT_PREVIEW_GEOMETRY,
        "conversation-artifacts" => ARTIFACTS_GEOMETRY,
        "conversation-environment" => ENVIRONMENT_GEOMETRY,
        "conversation-queue" => QUEUE_GEOMETRY,
        _ => panic!("unregistered reference scene {name}"),
    };
    SnapshotCase::new(name, WIDTH, HEIGHT, SCALE, geometry)
}

fn reference_cases() -> Vec<(SnapshotCase, ReferenceScene)> {
    reference_scenes(ThemePreference::Light, WIDTH)
        .into_iter()
        .map(|scene| (case_for(scene.name), scene))
        .collect()
}

#[test]
fn reference_scene_registry_has_required_landmarks() {
    assert_eq!(scene_names(), REFERENCE_SCENE_NAMES);
    assert_eq!(
        scene_names(),
        [
            "empty-task",
            "integrations-catalog",
            "settings-general",
            "conversation-document-preview",
            "conversation-artifacts",
            "conversation-environment",
            "conversation-queue",
        ]
    );

    for (case, scene) in reference_cases() {
        assert_scene_landmarks(&scene);
        assert_case_geometry(case, &scene.state);
    }
}

#[test]
fn reference_scenes_match_platform_goldens() {
    assert_eq!(scene_names(), REFERENCE_SCENE_NAMES);
    for (case, scene) in reference_cases()
        .into_iter()
        .filter(|(_, scene)| scene.name != "conversation-queue")
    {
        assert_scene_landmarks(&scene);
        assert_platform_snapshot(case, &scene.state);
    }
}

#[test]
fn conversation_queue_matches_platform_golden() {
    let scene = named_scene("conversation-queue", ThemePreference::Light, WIDTH)
        .expect("conversation queue scene is registered");
    assert_scene_landmarks(&scene);
    assert_platform_snapshot(case_for(scene.name), &scene.state);
}

fn assert_scene_landmarks(scene: &ReferenceScene) {
    let snapshot =
        WorkspaceSnapshot::build(&scene.state, WIDTH as f32, HEIGHT as f32, Insets::ZERO);
    match scene.name {
        "empty-task" => {
            assert!(scene.state.current_session.is_none());
            assert!(scene.state.transcripts.is_empty());
            assert!(scene.state.active_workspace.is_some());
            let active_workspace = scene.state.active_workspace.as_ref().unwrap();
            assert!(scene.state.threads.iter().any(|thread| {
                &thread.workspace_uri == active_workspace
                    && scene
                        .state
                        .presentation
                        .sessions
                        .get(&thread.session)
                        .and_then(|presentation| presentation.context.ready())
                        .is_some_and(|context| {
                            &context.workspace_uri == active_workspace
                                && context
                                    .branch
                                    .as_deref()
                                    .is_some_and(|branch| !branch.is_empty())
                        })
            }));
            let input = Composer::layout(snapshot.layout.composer, &scene.state.composer).input;
            let empty_height =
                input.origin.y - TRANSCRIPT_COMPOSER_GAP - snapshot.layout.transcript.origin.y;
            assert!((empty_height - 868.0).abs() <= 2.0);
            let gap = 10.0;
            let card_width = (snapshot.layout.transcript.width() - 24.0 - gap * 3.0) / 4.0;
            let suggestion_cards = [card_width; 4];
            assert_eq!(suggestion_cards.len(), 4);
            assert!(suggestion_cards
                .iter()
                .all(|width| (*width - 172.0).abs() <= 4.0));
            assert!((10.0..=12.0).contains(&gap));
        }
        "integrations-catalog" => {
            let catalog = scene
                .state
                .presentation
                .integrations
                .ready()
                .expect("integrations scene has a loaded local registry catalog");
            assert!(catalog.installed.len() >= 8);
            assert!(catalog.sections.len() >= 2);
            assert!(catalog.all_entries().count() >= 10);
            assert!(catalog
                .all_entries()
                .all(|entry| entry.source_id.is_some() || entry.fixture_only));
        }
        "settings-general" => {
            let layout = SettingsPanel::layout(
                snapshot.layout.sidebar,
                snapshot.layout.primary_surface,
                &scene.state,
            );
            assert!(layout.navigation.entries.len() >= 15);
            assert!(layout.general.permission_presets.len() >= 3);
            assert!(layout.general.general_rows.len() >= 8);
        }
        "conversation-document-preview" => {
            assert!(scene.block_count() >= 12);
            assert!(scene.visual_kinds().len() >= 5);
            assert_eq!(
                scene.state.presentation.secondary_pane,
                Some(SecondaryPane::DocumentPreview)
            );
            assert!(matches!(
                scene
                    .state
                    .current_session_presentation()
                    .map(|state| &state.preview),
                Some(PreviewState::Ready { .. })
            ));
            assert_eq!(snapshot.layout.review_panel.width(), 630.0);
        }
        "conversation-artifacts" => {
            assert!(scene.block_count() >= 12);
            assert!(scene.visual_kinds().len() >= 5);
            assert!(!scene.state.composer.attachments.is_empty());
            assert!(scene.state.presentation.pinned_summary_overlay_open);
            assert_eq!(snapshot.layout.context_panel.width(), 300.0);
            assert!(scene.state.composer.attachments.iter().all(|attachment| {
                !attachment.media_type.starts_with("data:")
                    && !attachment.display_name.contains("base64")
            }));
        }
        "conversation-environment" => {
            assert!(scene.block_count() >= 12);
            assert!(scene.visual_kinds().len() >= 5);
            assert!(scene.state.presentation.pinned_summary_overlay_open);
            assert!(environment_sections(&scene.state).len() >= 5);
            assert_eq!(snapshot.layout.context_panel.width(), 300.0);
        }
        "conversation-queue" => {
            let session = scene
                .state
                .current_session
                .as_ref()
                .expect("queue scene has a current session");
            let transcript = scene
                .state
                .transcripts
                .get(session)
                .expect("queue scene has a transcript");
            let queue = scene
                .state
                .message_queues
                .get(session)
                .expect("queue scene has pending messages");
            assert!(transcript.busy);
            assert_eq!(queue.items.len(), 4);
            assert!(state_thread_is_running(&scene.state, session));
            assert!(scene
                .state
                .composer
                .queue_menu
                .is_some_and(|id| queue.items.iter().any(|message| message.id == id)));
            let queue_layout = Composer::queue_layout(snapshot.layout.composer, &scene.state)
                .expect("queue scene lays out the queue surface");
            assert_eq!(queue_layout.rows.len(), 4);
            assert!(queue_layout.menu.is_some());
        }
        _ => panic!("unregistered reference scene {}", scene.name),
    }
}

fn state_thread_is_running(
    state: &zode_app_model::ZodeAppState,
    session: &zode_node_protocol::SessionLocator,
) -> bool {
    state.threads.iter().any(|thread| {
        &thread.session == session && thread.status == zode_node_protocol::ThreadStatus::Running
    })
}

/// Test-only rendering entry. It keeps rich fixtures unreachable from the
/// production binary while still providing a stable command for visual review:
///
/// `ZODE_RENDER_SCENE=empty-task ZODE_RENDER_PATH=target/empty-task.png \
///  cargo test -p zode-app --test snapshots render_named_test_scene -- --ignored --exact`
#[test]
#[ignore = "manual test-scene rendering entry"]
fn render_named_test_scene() {
    let name = std::env::var("ZODE_RENDER_SCENE").expect("ZODE_RENDER_SCENE is required");
    let requested =
        PathBuf::from(std::env::var("ZODE_RENDER_PATH").expect("ZODE_RENDER_PATH is required"));
    let path = if requested.is_absolute() {
        requested
    } else {
        workspace_root().join(requested)
    };
    let mut scene = named_scene(&name, ThemePreference::Light, WIDTH).unwrap_or_else(|| {
        panic!("unknown scene {name}; expected one of {REFERENCE_SCENE_NAMES:?}")
    });
    if std::env::var_os("ZODE_RENDER_PROJECT_PICKER").is_some() {
        scene.state.project_picker.open = true;
    }
    if std::env::var_os("ZODE_RENDER_PROJECTLESS").is_some() {
        scene.state.current_session = None;
        scene.state.active_workspace = None;
        scene.state.project_picker.open = false;
    }
    if std::env::var_os("ZODE_RENDER_SIDEBAR_REFERENCE").is_some() {
        populate_sidebar_reference(&mut scene.state);
    }
    let bytes = render_snapshot(&scene.state, WIDTH, HEIGHT, SCALE)
        .unwrap_or_else(|error| panic!("could not render {name}: {error}"));
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("could not create {}: {error}", parent.display()));
    }
    std::fs::write(&path, bytes)
        .unwrap_or_else(|error| panic!("could not write {}: {error}", path.display()));
    println!("rendered {name} to {}", path.display());
}

fn populate_sidebar_reference(state: &mut zode_app_model::ZodeAppState) {
    let node = state.host.node_id;
    let project = WorkspaceUri::new("file:///workspace/openpencil").unwrap();
    if let Some(thread) = state
        .threads
        .iter()
        .find(|thread| thread.title == "梳理桌面端实施计划")
    {
        state.archived_sessions.insert(thread.session.clone());
    }
    if let Some(project) = state
        .projects
        .iter_mut()
        .find(|project| project.workspace_uri.as_str().ends_with("/codex"))
    {
        project.expanded = false;
    }
    for (index, title) in [
        "电脑 我们的 RUST 版",
        "梳理配色系统缺口",
        "更新 Clash Verge 前置代理 IP",
    ]
    .into_iter()
    .enumerate()
    {
        let session = SessionLocator::new(node, format!("sidebar-pinned-{index}"));
        state.threads.push(ThreadSummary {
            session: session.clone(),
            workspace_uri: project.clone(),
            title: title.into(),
            updated_at_ms: 1_800_000_000_000 - index as i64,
            status: ThreadStatus::Idle,
        });
        state.pinned_sessions.insert(session);
    }
    for index in 0..6 {
        state.threads.push(ThreadSummary {
            session: SessionLocator::new(node, format!("sidebar-project-{index}")),
            workspace_uri: project.clone(),
            title: format!("OpenPencil 侧栏任务 {}", index + 1),
            updated_at_ms: 1_720_580_000_000 + index,
            status: ThreadStatus::Idle,
        });
    }
    let task_root = WorkspaceUri::new("file:///workspace/task-workspaces").unwrap();
    state.projectless_workspace_root = Some(task_root.clone());
    for (index, title) in ["Add pika pet", "了解 Codex CLI 生图"]
        .into_iter()
        .enumerate()
    {
        state.threads.push(ThreadSummary {
            session: SessionLocator::new(node, format!("sidebar-task-{index}")),
            workspace_uri: WorkspaceUri::new(format!(
                "{}/sidebar-task-{index}",
                task_root.as_str().trim_end_matches('/')
            ))
            .unwrap(),
            title: title.into(),
            updated_at_ms: 1_780_000_000_000 + index as i64,
            status: ThreadStatus::Idle,
        });
    }
}

/// Manual reference comparison entry used by compare-reference-snapshots.sh.
#[test]
#[ignore = "manual approved-reference comparison entry"]
fn compare_reference_snapshots() {
    let reference_root = required_directory("ZODE_REFERENCE_ROOT");
    let actual_root = required_directory("ZODE_ACTUAL_ROOT");
    let output_root = PathBuf::from(
        std::env::var("ZODE_REFERENCE_DIFF_ROOT").expect("ZODE_REFERENCE_DIFF_ROOT is required"),
    );
    for (scene, reference) in [
        ("empty-task", "06-empty-state.png"),
        ("integrations-catalog", "05-integrations.png"),
        ("settings-general", "04-settings.png"),
        ("conversation-document-preview", "03-editor-split.png"),
        ("conversation-artifacts", "02-artifacts-and-composer.png"),
        ("conversation-environment", "01-main-conversation.png"),
    ] {
        let diff = compare_reference_images(
            scene,
            &reference_root.join(reference),
            &actual_root.join(format!("{scene}.png")),
            &output_root,
        )
        .unwrap_or_else(|error| panic!("could not compare {scene}: {error}"));
        println!("{scene}: {diff:?}");
    }
}

fn required_directory(variable: &str) -> PathBuf {
    let path =
        PathBuf::from(std::env::var(variable).unwrap_or_else(|_| panic!("{variable} is required")));
    assert!(path.is_dir(), "{} must be a directory", path.display());
    path
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("zode-app lives below the workspace root")
        .to_path_buf()
}

#[test]
fn fixture_registry_is_test_only() {
    let production_sources = [
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bootstrap_state.rs"),
    ];
    for source in production_sources.into_iter().filter(|path| path.is_file()) {
        let text = std::fs::read_to_string(&source)
            .unwrap_or_else(|error| panic!("could not read {}: {error}", source.display()));
        assert!(!text.contains("snapshot_support"));
        assert!(!text.contains("named_scene("));
        assert!(!text.contains("REFERENCE_SCENE_NAMES"));
    }
}

#[test]
fn integration_scene_catalog_is_loaded_not_placeholder_state() {
    let scene = named_scene("integrations-catalog", ThemePreference::Light, WIDTH)
        .expect("integrations scene is registered");
    assert!(matches!(
        scene.state.presentation.integrations,
        LoadState::Ready(_)
    ));
}
