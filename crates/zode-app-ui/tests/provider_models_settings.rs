use accesskit::{Action, Role, Toggled};
use jian_widgets::{Point2D, Rect};
use zode_app_model::{
    demo_state, reduce_settings_command, AppCommand, LoadState, ProviderDraft, ProviderKindChoice,
    ProviderModelEntry, ProviderModelsStatus, ProviderSummary, SettingsCategory,
    SettingsCommandOutcome, ShellPage, ShellRoute,
};
use zode_app_ui::{
    provider_model_widget_id, provider_remove_widget_id, provider_widget_id, Insets, RectExt,
    SettingsPanel, WorkspaceSnapshot, PROVIDER_ADD_ID, PROVIDER_API_KEY_INPUT_ID,
    PROVIDER_CANCEL_ID, PROVIDER_DEFAULT_MODEL_INPUT_ID, PROVIDER_KIND_ANTHROPIC_ID,
    PROVIDER_KIND_OLLAMA_ID, PROVIDER_KIND_OPENAI_ID, PROVIDER_SAVE_ID, SETTINGS_ROOT_ID,
};

#[test]
fn provider_models_route_has_centered_layout_and_typed_commands() {
    let state = provider_state();
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let layout = SettingsPanel::layout(
        snapshot.layout.sidebar,
        snapshot.layout.primary_surface,
        &state,
    );

    assert_eq!(layout.content.size.x, 768.0);
    assert_eq!(layout.provider_models.providers_card.size.x, 768.0);
    assert_eq!(layout.provider_models.providers.len(), 1);
    assert_eq!(layout.provider_models.providers[0].rect.size.y, 60.0);
    assert!(layout.provider_models.editor.is_some());

    assert_eq!(
        SettingsPanel::command_for_widget(&state, PROVIDER_ADD_ID),
        Some(AppCommand::BeginProviderEdit {
            draft: ProviderDraft::default(),
            credential_configured: false,
        })
    );
    assert_eq!(
        SettingsPanel::command_for_widget(&state, provider_widget_id("anthropic-main")),
        Some(AppCommand::BeginProviderEdit {
            draft: ProviderDraft {
                provider_id: "anthropic-main".into(),
                kind: ProviderKindChoice::Anthropic,
                base_url: Some("https://api.example.test".into()),
                model_ids: vec!["claude-sonnet".into(), "claude-opus".into()],
                default_model: Some("claude-sonnet".into()),
            },
            credential_configured: true,
        })
    );
    assert_eq!(
        SettingsPanel::command_for_widget(&state, provider_remove_widget_id("anthropic-main")),
        Some(AppCommand::RequestRemoveProvider {
            provider_id: "anthropic-main".into(),
        })
    );
    assert_eq!(
        SettingsPanel::command_for_widget(&state, PROVIDER_KIND_OPENAI_ID),
        Some(AppCommand::SetProviderDraftKind(ProviderKindChoice::OpenAi))
    );
    assert_eq!(
        SettingsPanel::command_for_widget(&state, provider_model_widget_id("claude-opus")),
        Some(AppCommand::SelectProviderDefaultModel("claude-opus".into()))
    );
    assert_eq!(
        SettingsPanel::command_for_widget(&state, PROVIDER_SAVE_ID),
        Some(AppCommand::RequestSaveProvider)
    );
    assert_eq!(
        SettingsPanel::command_for_widget(&state, PROVIDER_CANCEL_ID),
        Some(AppCommand::CancelProviderEdit)
    );
}

#[test]
fn provider_editor_a11y_never_exposes_secret_and_uses_model_radios() {
    let mut state = provider_state();
    state.provider_models.status = ProviderModelsStatus::Failed {
        provider_id: Some("anthropic-main".into()),
        message: "sentinel-secret-must-not-escape".into(),
    };
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);

    let api_key = snapshot
        .node(PROVIDER_API_KEY_INPUT_ID)
        .expect("API key input semantic node");
    assert_eq!(api_key.role, Role::TextInput);
    assert_eq!(api_key.name, "API Key");
    assert_eq!(api_key.value, None);
    assert!(api_key.actions.contains(&Action::Focus));
    assert!(api_key.actions.contains(&Action::SetValue));

    assert!(snapshot.node(PROVIDER_DEFAULT_MODEL_INPUT_ID).is_none());
    let selected_model = snapshot
        .node(provider_model_widget_id("claude-sonnet"))
        .expect("selected model radio");
    assert_eq!(selected_model.role, Role::RadioButton);
    assert_eq!(selected_model.toggled, Some(Toggled::True));
    assert!(selected_model.actions.contains(&Action::Click));
    assert!(selected_model.actions.contains(&Action::Focus));

    for id in [
        PROVIDER_KIND_ANTHROPIC_ID,
        PROVIDER_KIND_OPENAI_ID,
        PROVIDER_KIND_OLLAMA_ID,
    ] {
        assert_eq!(snapshot.node(id).unwrap().role, Role::RadioButton);
    }
    assert!(snapshot.nodes.iter().all(|node| {
        !node.name.contains("sentinel-secret-must-not-escape")
            && !node
                .value
                .as_deref()
                .is_some_and(|value| value.contains("sentinel-secret-must-not-escape"))
    }));
}

#[test]
fn provider_model_editor_scrolls_to_every_default_model_choice() {
    let mut state = provider_state();
    let draft = state.provider_models.editor.as_mut().unwrap();
    draft.model_ids = (0..18).map(|index| format!("model-{index:02}")).collect();
    draft.default_model = Some("model-17".into());

    let top = WorkspaceSnapshot::build(&state, 900.0, 480.0, Insets::ZERO);
    let content = SettingsPanel::page_layout(top.layout.primary_surface).0;
    assert!(SettingsPanel::max_scroll_offset(content, &state) > 0.0);
    assert!(top
        .node(SETTINGS_ROOT_ID)
        .unwrap()
        .actions
        .contains(&Action::ScrollDown));
    assert!(top.node(provider_model_widget_id("model-17")).is_none());

    let scroll = SettingsPanel::scroll_command(content, &state, 10_000.0);
    assert_eq!(
        reduce_settings_command(&mut state, scroll),
        SettingsCommandOutcome::Applied
    );
    let bottom = WorkspaceSnapshot::build(&state, 900.0, 480.0, Insets::ZERO);
    let last = bottom
        .node(provider_model_widget_id("model-17"))
        .expect("last model is reachable after scrolling");
    assert!(last.rect.max_y() <= content.max_y());
    assert_eq!(bottom.hit_test(rect_center(last.rect)), Some(last.id));
    assert!(bottom
        .node(SETTINGS_ROOT_ID)
        .unwrap()
        .actions
        .contains(&Action::ScrollUp));
}

fn provider_state() -> zode_app_model::ZodeAppState {
    let mut state = demo_state();
    state.shell.page = ShellPage::Settings;
    state.presentation.route = ShellRoute::Settings(SettingsCategory::ProviderModels);
    state.provider_models.load = LoadState::Ready(vec![ProviderSummary {
        provider_id: "anthropic-main".into(),
        kind: ProviderKindChoice::Anthropic,
        base_url: Some("https://api.example.test".into()),
        models: vec![
            ProviderModelEntry {
                model_id: "claude-sonnet".into(),
            },
            ProviderModelEntry {
                model_id: "claude-opus".into(),
            },
        ],
        default_model: Some("claude-sonnet".into()),
        credential_configured: true,
    }]);
    state.provider_models.editor = Some(ProviderDraft {
        provider_id: "anthropic-main".into(),
        kind: ProviderKindChoice::Anthropic,
        base_url: Some("https://api.example.test".into()),
        model_ids: vec!["claude-sonnet".into(), "claude-opus".into()],
        default_model: Some("claude-sonnet".into()),
    });
    state.provider_models.credential_configured = true;
    state
}

fn rect_center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}
