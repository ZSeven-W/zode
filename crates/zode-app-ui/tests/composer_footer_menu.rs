use accesskit::{Action, Role};
use jian_widgets::{Point2D, Rect};
use zode_app_model::{
    demo_state, AppCommand, ComposerFooterMenu, SettingsCategory, ShellRoute, ZodeAppState,
};
use zode_app_ui::{
    Composer, ComposerFooterMenuWidget, Insets, RectExt, WorkspaceSnapshot, COMPOSER_ADD_FILE_ID,
    COMPOSER_ADD_GOAL_ID, COMPOSER_ADD_ID, COMPOSER_ADD_PLAN_ID, COMPOSER_ADD_WECHAT_ID,
    COMPOSER_MODEL_ADD_ID, COMPOSER_MODEL_CONFIGURE_ID, COMPOSER_MODEL_EFFORTS_ID,
    COMPOSER_MODEL_EFFORT_HIGH_ID, COMPOSER_MODEL_ID, COMPOSER_MODEL_MODELS_ID,
    COMPOSER_MODEL_RESET_ID, COMPOSER_MODEL_SPEEDS_ID, COMPOSER_MODEL_SPEED_ID,
    COMPOSER_PERMISSION_CUSTOM_ID, COMPOSER_PERMISSION_FULL_ID, COMPOSER_PERMISSION_ID,
    COMPOSER_PERMISSION_REQUEST_ID,
};
use zode_node_protocol::{ApprovalMode, RuntimeOptions, SandboxMode, SessionLocator, TurnId};

fn input(state: &ZodeAppState) -> Rect {
    Composer::layout_for_state(Rect::xywh(280.0, 720.0, 760.0, 150.0), state).input
}

fn menu(state: &ZodeAppState) -> zode_app_ui::ComposerFooterMenuLayout {
    ComposerFooterMenuWidget::layout(Rect::xywh(0.0, 0.0, 1_200.0, 900.0), input(state), state)
        .expect("footer menu is open")
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

#[test]
fn footer_menu_clamps_safely_inside_a_narrow_viewport() {
    let mut state = demo_state();
    state.composer.footer_menu = Some(ComposerFooterMenu::Add);
    let viewport = Rect::xywh(0.0, 0.0, 100.0, 900.0);
    let layout = ComposerFooterMenuWidget::layout(viewport, input(&state), &state)
        .expect("footer menu is open");

    assert_eq!(layout.surface.origin.x, 8.0);
    assert_eq!(layout.surface.size.x, 84.0);
    assert!(layout.surface.max_x() <= viewport.max_x() - 8.0);
}

#[test]
fn permission_menu_maps_real_presets_and_routes_custom_configuration() {
    let mut state = demo_state();
    state.composer.footer_menu = Some(ComposerFooterMenu::Permission);
    let layout = menu(&state);

    assert_eq!(layout.sections[0].label, "应如何批准 Zode 操作？");
    assert_eq!(
        ComposerFooterMenuWidget::command_for_widget(&state, COMPOSER_PERMISSION_REQUEST_ID),
        Some(AppCommand::SetPermissionPreset {
            approval_mode: ApprovalMode::Request,
            sandbox_mode: SandboxMode::WorkspaceWrite,
            network: false,
        })
    );
    assert_eq!(
        ComposerFooterMenuWidget::command_for_widget(&state, COMPOSER_PERMISSION_FULL_ID),
        Some(AppCommand::SetPermissionPreset {
            approval_mode: ApprovalMode::Full,
            sandbox_mode: SandboxMode::Off,
            network: false,
        })
    );
    assert_eq!(
        ComposerFooterMenuWidget::command_for_widget(&state, COMPOSER_PERMISSION_CUSTOM_ID),
        Some(AppCommand::Navigate(ShellRoute::Settings(
            SettingsCategory::Configuration,
        )))
    );
    let custom = layout
        .rows
        .iter()
        .find(|row| row.id == COMPOSER_PERMISSION_CUSTOM_ID)
        .unwrap();
    assert_eq!(
        custom.detail.as_deref(),
        Some("使用 ~/.zode/config.json 中定义的权限")
    );
}

#[test]
fn add_menu_exposes_planned_actions_without_fake_click_targets() {
    let mut state = demo_state();
    state.composer.footer_menu = Some(ComposerFooterMenu::Add);
    let layout = menu(&state);
    assert_eq!(layout.surface.size.x, input(&state).size.x);

    for (id, detail) in [
        (COMPOSER_ADD_FILE_ID, "当前仅支持从剪贴板粘贴图片"),
        (COMPOSER_ADD_WECHAT_ID, "当前版本尚未接入"),
        (COMPOSER_ADD_GOAL_ID, "当前版本尚未接入"),
        (COMPOSER_ADD_PLAN_ID, "当前版本尚未接入"),
    ] {
        let row = layout.rows.iter().find(|row| row.id == id).unwrap();
        assert!(!row.enabled);
        assert_eq!(row.detail.as_deref(), Some(detail));
        assert_eq!(
            ComposerFooterMenuWidget::command_for_widget(&state, id),
            None
        );
    }
}

#[test]
fn unconfigured_builtin_codex_stays_actionable_and_runtime_gaps_are_explicit() {
    let mut state = demo_state();
    state.provider_setup_required = true;
    state.composer.footer_menu = Some(ComposerFooterMenu::Model);
    state.composer_defaults = Some(RuntimeOptions {
        models: vec!["codex-test".into()],
        active_model: Some("codex-test".into()),
        effort: Some("high".into()),
        approval_mode: Default::default(),
        sandbox_mode: SandboxMode::WorkspaceWrite,
        sandbox_network: false,
    });
    let root = menu(&state);
    assert_eq!(root.rows[0].id, COMPOSER_MODEL_MODELS_ID);
    assert_eq!(root.rows[1].id, COMPOSER_MODEL_EFFORTS_ID);
    assert_eq!(root.rows[2].id, COMPOSER_MODEL_SPEEDS_ID);
    assert_eq!(
        ComposerFooterMenuWidget::command_for_widget(&state, COMPOSER_MODEL_MODELS_ID),
        Some(AppCommand::ToggleComposerFooterMenu(
            ComposerFooterMenu::ModelModels,
        ))
    );

    state.composer.footer_menu = Some(ComposerFooterMenu::ModelModels);
    let layout = menu(&state);
    let configure = layout
        .rows
        .iter()
        .find(|row| row.id == COMPOSER_MODEL_CONFIGURE_ID)
        .unwrap();
    assert!(configure.enabled);
    assert_eq!(configure.label, "Codex（内置）· 尚未配置");
    assert_eq!(
        ComposerFooterMenuWidget::command_for_widget(&state, configure.id),
        Some(AppCommand::Navigate(ShellRoute::Settings(
            SettingsCategory::ProviderModels,
        )))
    );

    state.composer.footer_menu = Some(ComposerFooterMenu::ModelSpeed);
    let layout = menu(&state);
    let speed = layout
        .rows
        .iter()
        .find(|row| row.id == COMPOSER_MODEL_SPEED_ID)
        .unwrap();

    assert!(!speed.enabled);
    assert_eq!(speed.detail.as_deref(), Some("当前仅支持标准速度"));
    assert_eq!(
        ComposerFooterMenuWidget::command_for_widget(&state, COMPOSER_MODEL_SPEED_ID),
        None
    );
    state.composer.footer_menu = Some(ComposerFooterMenu::Model);
    assert_eq!(
        ComposerFooterMenuWidget::command_for_widget(&state, COMPOSER_MODEL_RESET_ID),
        Some(AppCommand::ResetComposerRuntime)
    );
}

#[test]
fn configured_model_menu_always_exposes_add_model_route_at_the_bottom() {
    let mut state = demo_state();
    state.provider_setup_required = false;
    state.composer.available_models = vec!["gpt-5.6-sol".into(), "gpt-5.6-codex".into()];
    state.composer.model = Some("gpt-5.6-sol".into());
    state.composer.footer_menu = Some(ComposerFooterMenu::ModelModels);

    let layout = menu(&state);
    let add = layout.rows.last().expect("add-model row");
    assert_eq!(add.id, COMPOSER_MODEL_ADD_ID);
    assert_eq!(add.label, "添加模型");
    assert!(add.enabled);
    assert_eq!(
        ComposerFooterMenuWidget::command_for_widget(&state, add.id),
        Some(AppCommand::Navigate(ShellRoute::Settings(
            SettingsCategory::ProviderModels,
        )))
    );

    let snapshot = WorkspaceSnapshot::build(&state, 1_200.0, 900.0, Insets::ZERO);
    let add_node = snapshot
        .node(COMPOSER_MODEL_ADD_ID)
        .expect("accessible add-model row");
    assert_eq!(add_node.role, Role::MenuItem);
    assert!(add_node.actions.contains(&Action::Click));
    assert!(add_node.actions.contains(&Action::Focus));
    assert_eq!(
        snapshot.hit_test(center(add_node.rect)),
        Some(COMPOSER_MODEL_ADD_ID)
    );
}

#[test]
fn active_turn_locks_runtime_mutations_but_keeps_setup_guidance_available() {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "active-footer");
    state.current_session = Some(session.clone());
    state.active_turns.insert(session, TurnId::new());
    state.provider_setup_required = true;
    state.composer.available_models = vec!["codex-test".into()];
    state.composer.footer_menu = Some(ComposerFooterMenu::ModelEffort);
    let layout = menu(&state);

    let effort = layout
        .rows
        .iter()
        .find(|row| row.id == COMPOSER_MODEL_EFFORT_HIGH_ID)
        .unwrap();
    assert!(!effort.enabled);
    assert_eq!(effort.detail.as_deref(), Some("任务运行中，完成后可更改"));
    assert_eq!(
        ComposerFooterMenuWidget::command_for_widget(&state, effort.id),
        None
    );
    state.composer.footer_menu = Some(ComposerFooterMenu::ModelModels);
    let model_layout = menu(&state);
    assert!(
        model_layout
            .rows
            .iter()
            .find(|row| row.id == COMPOSER_MODEL_CONFIGURE_ID)
            .unwrap()
            .enabled
    );

    state.composer.footer_menu = Some(ComposerFooterMenu::Permission);
    let layout = menu(&state);
    assert!(layout.rows.iter().all(|row| !row.enabled));
    assert!(layout
        .rows
        .iter()
        .all(|row| row.detail.as_deref() == Some("任务运行中，完成后可更改")));
}

#[test]
fn composer_accessibility_has_three_footer_buttons_and_no_microphone() {
    let snapshot = WorkspaceSnapshot::build(&demo_state(), 1_200.0, 900.0, Insets::ZERO);

    for id in [COMPOSER_ADD_ID, COMPOSER_PERMISSION_ID, COMPOSER_MODEL_ID] {
        let trigger = snapshot.node(id).expect("footer trigger");
        assert_eq!(snapshot.hit_test(center(trigger.rect)), Some(id));
    }
    assert!(snapshot
        .nodes
        .iter()
        .all(|node| !node.name.contains("麦克风")));
}
