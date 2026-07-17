use super::*;
use zode_app_model::{demo_state, ProviderModelsStatus};

fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "zode-provider-models-{name}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).expect("test config directory");
    dir
}

fn begin_edit(
    controller: &mut ProviderModelsController,
    state: &mut ZodeAppState,
    draft: ProviderDraft,
    credential_configured: bool,
) {
    let outcome = controller.consume(
        state,
        AppCommand::BeginProviderEdit {
            draft,
            credential_configured,
        },
    );
    assert!(matches!(outcome, ProviderModelsCommandOutcome::Handled));
}

fn draft(provider_id: &str, models: &[&str], default_model: &str) -> ProviderDraft {
    ProviderDraft {
        provider_id: provider_id.into(),
        kind: ProviderKindChoice::OpenAi,
        base_url: Some("https://example.test/v1".into()),
        model_ids: models.iter().map(|model| (*model).into()).collect(),
        default_model: Some(default_model.into()),
    }
}

#[test]
fn invalid_draft_fails_without_persisting_or_panicking() {
    let dir = test_dir("invalid-draft");
    let mut controller = ProviderModelsController::with_config_dir(dir.clone());
    let mut state = demo_state();
    begin_edit(&mut controller, &mut state, ProviderDraft::default(), false);

    assert!(matches!(
        controller.consume(&mut state, AppCommand::RequestSaveProvider),
        ProviderModelsCommandOutcome::Handled
    ));
    assert!(matches!(
        state.provider_models.status,
        ProviderModelsStatus::Failed { .. }
    ));
    assert!(!ConfigManager::global_path_in(&dir).exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn changing_provider_kind_does_not_reuse_the_old_credential() {
    let dir = test_dir("kind-change");
    let mut config = ZodeConfig::default();
    config.providers.insert(
        "cloud".into(),
        ProviderConfig {
            r#type: Some(ProviderKind::Openai),
            api_key: Some("openai-secret".into()),
            model: Some("model-a".into()),
            ..Default::default()
        },
    );
    config.set_active_model("model-a");
    ConfigManager::save_global_in(&dir, &config).expect("seed config");
    let before = std::fs::read(ConfigManager::global_path_in(&dir)).expect("seed bytes");

    let mut controller = ProviderModelsController::with_config_dir(dir.clone());
    let mut state = demo_state();
    controller.refresh(&mut state);
    let edited = ProviderDraft {
        provider_id: "cloud".into(),
        kind: ProviderKindChoice::Anthropic,
        base_url: Some("https://example.test/v1".into()),
        model_ids: vec!["model-a".into()],
        default_model: Some("model-a".into()),
    };
    begin_edit(&mut controller, &mut state, edited, true);
    assert!(matches!(
        controller.consume(&mut state, AppCommand::RequestSaveProvider),
        ProviderModelsCommandOutcome::Handled
    ));
    assert!(matches!(
        &state.provider_models.status,
        ProviderModelsStatus::Failed { message, .. } if message.contains("ANTHROPIC_API_KEY")
    ));
    let after = std::fs::read(ConfigManager::global_path_in(&dir)).expect("config bytes");
    assert_eq!(after, before);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_round_trip_preserves_unrelated_config_and_model_overrides() {
    let dir = test_dir("round-trip");
    let mut config = ZodeConfig {
        theme: Some("dark".into()),
        language: Some("zh".into()),
        ..Default::default()
    };
    let mut provider = ProviderConfig {
        r#type: Some(ProviderKind::Openai),
        api_key: Some("old-secret".into()),
        base_url: Some("https://old.test/v1".into()),
        ..Default::default()
    };
    provider.models.insert(
        "model-a".into(),
        ModelOverride {
            context_window: Some(128_000),
            ..Default::default()
        },
    );
    config.providers.insert("cloud".into(), provider);
    config.set_active_model("model-a");
    ConfigManager::save_global_in(&dir, &config).expect("seed config");

    let mut controller = ProviderModelsController::with_config_dir(dir.clone());
    let mut state = demo_state();
    controller.refresh(&mut state);
    begin_edit(
        &mut controller,
        &mut state,
        draft("cloud", &["model-a", "model-b"], "model-b"),
        true,
    );
    for _ in 0..2 {
        assert!(matches!(
            controller.consume(&mut state, AppCommand::RequestSaveProvider),
            ProviderModelsCommandOutcome::Forward(AppCommand::ReloadProviderConfiguration)
        ));
    }

    let saved = ConfigManager::load_global_in(&dir).expect("saved config");
    assert_eq!(saved.theme.as_deref(), Some("dark"));
    assert_eq!(saved.language.as_deref(), Some("zh"));
    assert_eq!(
        saved.providers["cloud"].api_key.as_deref(),
        Some("old-secret")
    );
    assert_eq!(
        saved.providers["cloud"].models["model-a"].context_window,
        Some(128_000)
    );
    assert_eq!(
        saved.providers["cloud"]
            .models
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["model-a", "model-b"]
    );
    assert_eq!(saved.provider.model.as_deref(), Some("model-b"));
    assert!(controller.secret_input_value().is_empty());
    assert!(!format!("{state:?}").contains("old-secret"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn new_secret_replaces_old_secret_without_entering_application_state() {
    let dir = test_dir("replace-secret");
    let mut config = ZodeConfig::default();
    config.providers.insert(
        "cloud".into(),
        ProviderConfig {
            r#type: Some(ProviderKind::Openai),
            api_key: Some("old-secret".into()),
            model: Some("model-a".into()),
            ..Default::default()
        },
    );
    config.set_active_model("model-a");
    ConfigManager::save_global_in(&dir, &config).expect("seed config");

    let mut controller = ProviderModelsController::with_config_dir(dir.clone());
    let mut state = demo_state();
    controller.refresh(&mut state);
    begin_edit(
        &mut controller,
        &mut state,
        draft("cloud", &["model-a"], "model-a"),
        true,
    );
    assert!(controller.set_secret_input_value("new-secret".into(), ProviderKindChoice::OpenAi));
    assert!(!format!("{state:?}").contains("new-secret"));
    let _ = controller.consume(&mut state, AppCommand::RequestSaveProvider);
    let saved = ConfigManager::load_global_in(&dir).expect("saved config");
    assert_eq!(
        saved.providers["cloud"].api_key.as_deref(),
        Some("new-secret")
    );
    assert!(!format!("{state:?}").contains("new-secret"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn missing_required_credential_returns_chinese_error_without_writing() {
    let dir = test_dir("missing-key");
    let mut controller = ProviderModelsController::with_config_dir(dir.clone());
    let mut state = demo_state();
    begin_edit(
        &mut controller,
        &mut state,
        draft("cloud", &["model-a"], "model-a"),
        false,
    );
    assert!(matches!(
        controller.consume(&mut state, AppCommand::RequestSaveProvider),
        ProviderModelsCommandOutcome::Handled
    ));
    assert!(matches!(
        &state.provider_models.status,
        ProviderModelsStatus::Failed { message, .. } if message.contains("API Key")
    ));
    assert!(!ConfigManager::global_path_in(&dir).exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn removing_active_provider_selects_first_remaining_model() {
    let dir = test_dir("remove");
    let mut config = ZodeConfig::default();
    for (provider_id, model_id) in [("first", "model-a"), ("second", "model-b")] {
        let mut provider = ProviderConfig {
            r#type: Some(ProviderKind::Ollama),
            ..Default::default()
        };
        provider
            .models
            .insert(model_id.into(), ModelOverride::default());
        config.providers.insert(provider_id.into(), provider);
    }
    config.set_active_model("model-a");
    ConfigManager::save_global_in(&dir, &config).expect("seed config");

    let mut controller = ProviderModelsController::with_config_dir(dir.clone());
    let mut state = demo_state();
    controller.refresh(&mut state);
    assert!(matches!(
        controller.consume(
            &mut state,
            AppCommand::RequestRemoveProvider {
                provider_id: "first".into()
            }
        ),
        ProviderModelsCommandOutcome::Forward(AppCommand::ReloadProviderConfiguration)
    ));
    let saved = ConfigManager::load_global_in(&dir).expect("saved config");
    assert!(!saved.providers.contains_key("first"));
    assert_eq!(saved.provider.model.as_deref(), Some("model-b"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn legacy_flat_config_is_normalized_before_listing_and_saving() {
    let dir = test_dir("legacy-flat");
    let path = ConfigManager::global_path_in(&dir);
    std::fs::write(
        &path,
        r#"{
            "anthropic_api_key": "legacy-secret",
            "model": "claude-sonnet-4-6",
            "theme": "dark"
        }"#,
    )
    .expect("legacy config");

    let mut controller = ProviderModelsController::with_config_dir(dir.clone());
    let mut state = demo_state();
    controller.refresh(&mut state);
    let LoadState::Ready(summaries) = &state.provider_models.load else {
        panic!("provider summaries should load");
    };
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].provider_id, "anthropic");
    assert!(summaries[0].credential_configured);
    assert_eq!(summaries[0].models[0].model_id, "claude-sonnet-4-6");

    begin_edit(
        &mut controller,
        &mut state,
        ProviderDraft {
            provider_id: "anthropic".into(),
            kind: ProviderKindChoice::Anthropic,
            base_url: None,
            model_ids: vec!["claude-sonnet-4-6".into()],
            default_model: Some("claude-sonnet-4-6".into()),
        },
        true,
    );
    assert!(matches!(
        controller.consume(&mut state, AppCommand::RequestSaveProvider),
        ProviderModelsCommandOutcome::Forward(AppCommand::ReloadProviderConfiguration)
    ));
    let saved = ConfigManager::load_global_in(&dir).expect("migrated config");
    assert_eq!(saved.theme.as_deref(), Some("dark"));
    assert_eq!(
        saved.providers["anthropic"].api_key.as_deref(),
        Some("legacy-secret")
    );
    let serialized = std::fs::read_to_string(path).expect("serialized config");
    assert!(!serialized.contains("anthropic_api_key"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pending_secret_len_counts_unicode_chars_and_resets_on_begin_and_save() {
    let dir = test_dir("pending-secret-len");
    let mut controller = ProviderModelsController::with_config_dir(dir.clone());
    let mut state = demo_state();
    begin_edit(
        &mut controller,
        &mut state,
        draft("cloud", &["model-a"], "model-a"),
        false,
    );
    assert_eq!(controller.pending_secret_len(), 0);

    // Each of these three characters is multi-byte in UTF-8, but must still
    // count as one character each, not one per byte.
    controller.set_secret_input_value("密钥x".into(), ProviderKindChoice::OpenAi);
    assert_eq!(controller.pending_secret_len(), 3);

    // Beginning a fresh edit clears the previous buffer.
    begin_edit(
        &mut controller,
        &mut state,
        draft("cloud", &["model-a"], "model-a"),
        false,
    );
    assert_eq!(controller.pending_secret_len(), 0);

    controller.set_secret_input_value("sk-live-secret".into(), ProviderKindChoice::OpenAi);
    assert_eq!(controller.pending_secret_len(), 14);
    let _ = controller.consume(&mut state, AppCommand::RequestSaveProvider);
    assert_eq!(controller.pending_secret_len(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn switching_away_from_openai_clears_incompatible_dialect() {
    let dir = test_dir("dialect-change");
    let mut config = ZodeConfig::default();
    config.providers.insert(
        "cloud".into(),
        ProviderConfig {
            r#type: Some(ProviderKind::Openai),
            api_key: Some("openai-secret".into()),
            model: Some("model-a".into()),
            dialect: Some("deepseek".into()),
            ..Default::default()
        },
    );
    config.set_active_model("model-a");
    ConfigManager::save_global_in(&dir, &config).expect("seed config");

    let mut controller = ProviderModelsController::with_config_dir(dir.clone());
    let mut state = demo_state();
    controller.refresh(&mut state);
    begin_edit(
        &mut controller,
        &mut state,
        ProviderDraft {
            provider_id: "cloud".into(),
            kind: ProviderKindChoice::Anthropic,
            base_url: None,
            model_ids: vec!["model-a".into()],
            default_model: Some("model-a".into()),
        },
        true,
    );
    assert!(
        controller.set_secret_input_value("anthropic-secret".into(), ProviderKindChoice::Anthropic)
    );
    let _ = controller.consume(&mut state, AppCommand::RequestSaveProvider);
    let saved = ConfigManager::load_global_in(&dir).expect("saved config");
    assert_eq!(saved.providers["cloud"].kind(), ProviderKind::Anthropic);
    assert!(saved.providers["cloud"].dialect.is_none());
    let _ = std::fs::remove_dir_all(dir);
}
