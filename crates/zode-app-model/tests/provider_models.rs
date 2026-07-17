use zode_app_model::{
    demo_state, reduce_presentation_command, reduce_settings_command, AppCommand, LoadState,
    PresentationCommandOutcome, ProviderDraft, ProviderKindChoice, ProviderModelEntry,
    ProviderModelsStatus, ProviderSummary, SettingsCategory, SettingsCommandOutcome, ShellRoute,
};

fn draft(provider_id: &str) -> ProviderDraft {
    ProviderDraft {
        provider_id: provider_id.into(),
        kind: ProviderKindChoice::OpenAi,
        base_url: Some("https://api.example.test/v1".into()),
        model_ids: vec!["model-a".into(), "model-b".into()],
        default_model: Some("model-a".into()),
    }
}

fn summary(provider_id: &str, credential_configured: bool) -> ProviderSummary {
    ProviderSummary {
        provider_id: provider_id.into(),
        kind: ProviderKindChoice::OpenAi,
        base_url: Some("https://api.example.test/v1".into()),
        models: vec![
            ProviderModelEntry {
                model_id: "model-a".into(),
            },
            ProviderModelEntry {
                model_id: "model-b".into(),
            },
        ],
        default_model: Some("model-a".into()),
        credential_configured,
    }
}

#[test]
fn provider_models_is_a_typed_settings_destination() {
    let mut state = demo_state();

    assert_eq!(
        reduce_presentation_command(
            &mut state,
            AppCommand::SelectSettingsCategory(SettingsCategory::ProviderModels),
        ),
        PresentationCommandOutcome::Applied,
    );
    assert_eq!(
        state.presentation.route,
        ShellRoute::Settings(SettingsCategory::ProviderModels)
    );
}

#[test]
fn provider_draft_contains_only_non_secret_configuration() {
    let draft = ProviderDraft {
        provider_id: "codex".into(),
        kind: ProviderKindChoice::Anthropic,
        base_url: None,
        model_ids: vec!["claude-sonnet".into()],
        default_model: Some("claude-sonnet".into()),
    };

    assert_eq!(draft.provider_id, "codex");
    assert_eq!(draft.kind, ProviderKindChoice::Anthropic);
    assert_eq!(draft.model_ids, vec!["claude-sonnet"]);
}

#[test]
fn provider_catalog_load_is_explicit_and_normalizes_safe_metadata() {
    let mut state = demo_state();

    assert_eq!(
        reduce_settings_command(&mut state, AppCommand::LoadProviderModels),
        SettingsCommandOutcome::Applied,
    );
    assert_eq!(state.provider_models.load, LoadState::Loading);

    let loaded = ProviderSummary {
        provider_id: "  codex  ".into(),
        kind: ProviderKindChoice::Anthropic,
        base_url: Some("   ".into()),
        models: vec![
            ProviderModelEntry {
                model_id: " model-a ".into(),
            },
            ProviderModelEntry {
                model_id: "model-a".into(),
            },
            ProviderModelEntry {
                model_id: " ".into(),
            },
        ],
        default_model: Some("missing".into()),
        credential_configured: true,
    };
    assert_eq!(
        reduce_settings_command(&mut state, AppCommand::ProviderModelsLoaded(vec![loaded])),
        SettingsCommandOutcome::Applied,
    );

    let LoadState::Ready(providers) = &state.provider_models.load else {
        panic!("provider metadata should be ready");
    };
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].provider_id, "codex");
    assert_eq!(providers[0].base_url, None);
    assert_eq!(
        providers[0].models,
        vec![ProviderModelEntry {
            model_id: "model-a".into()
        }]
    );
    assert_eq!(providers[0].default_model, None);
    assert!(providers[0].credential_configured);

    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::ProviderModelsLoadFailed("offline".into()),
        ),
        SettingsCommandOutcome::Applied,
    );
    assert_eq!(
        state.provider_models.load,
        LoadState::Failed("offline".into())
    );
}

#[test]
fn editor_updates_only_non_secret_fields_and_default_must_exist() {
    let mut state = demo_state();

    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::BeginProviderEdit {
                draft: draft("codex"),
                credential_configured: true,
            },
        ),
        SettingsCommandOutcome::Applied,
    );
    assert!(state.provider_models.credential_configured);
    assert_eq!(state.provider_models.pending_secret_len, 0);

    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::SetProviderDraftKind(ProviderKindChoice::Ollama),
        ),
        SettingsCommandOutcome::Applied,
    );
    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::SetProviderDraftBaseUrl(Some("http://localhost:11434".into())),
        ),
        SettingsCommandOutcome::Applied,
    );
    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::SetProviderDraftModelIds(vec!["llama3".into()]),
        ),
        SettingsCommandOutcome::Applied,
    );
    assert_eq!(
        state.provider_models.editor.as_ref().unwrap().default_model,
        None
    );
    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::SelectProviderDefaultModel("missing".into()),
        ),
        SettingsCommandOutcome::Ignored,
    );
    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::SelectProviderDefaultModel("llama3".into()),
        ),
        SettingsCommandOutcome::Applied,
    );

    let editor = state.provider_models.editor.as_ref().unwrap();
    assert_eq!(editor.kind, ProviderKindChoice::Ollama);
    assert_eq!(editor.base_url.as_deref(), Some("http://localhost:11434"));
    assert_eq!(editor.default_model.as_deref(), Some("llama3"));
}

#[test]
fn save_request_validates_normalizes_and_upserts_canonical_summary() {
    let mut state = demo_state();
    let invalid = ProviderDraft {
        provider_id: " ".into(),
        kind: ProviderKindChoice::Anthropic,
        base_url: None,
        model_ids: Vec::new(),
        default_model: None,
    };
    reduce_settings_command(
        &mut state,
        AppCommand::BeginProviderEdit {
            draft: invalid,
            credential_configured: false,
        },
    );
    assert_eq!(
        reduce_settings_command(&mut state, AppCommand::RequestSaveProvider),
        SettingsCommandOutcome::Applied,
    );
    assert!(matches!(
        state.provider_models.status,
        ProviderModelsStatus::Failed {
            provider_id: None,
            ..
        }
    ));

    let valid = ProviderDraft {
        provider_id: " codex ".into(),
        kind: ProviderKindChoice::OpenAi,
        base_url: Some(" https://api.example.test/v1 ".into()),
        model_ids: vec![" model-a ".into(), "model-a".into(), "model-b".into()],
        default_model: Some(" model-b ".into()),
    };
    assert_eq!(
        reduce_settings_command(&mut state, AppCommand::UpdateProviderDraft(valid)),
        SettingsCommandOutcome::Applied,
    );
    assert_eq!(
        reduce_settings_command(&mut state, AppCommand::RequestSaveProvider),
        SettingsCommandOutcome::Applied,
    );
    assert_eq!(
        state.provider_models.status,
        ProviderModelsStatus::Saving {
            provider_id: "codex".into()
        }
    );
    let editor = state.provider_models.editor.as_ref().unwrap();
    assert_eq!(editor.model_ids, vec!["model-a", "model-b"]);
    assert_eq!(editor.default_model.as_deref(), Some("model-b"));

    // Simulate a few keystrokes typed into the API-key field while saving is
    // in flight; a successful save must clear this projection.
    state.provider_models.pending_secret_len = 7;

    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::ProviderSaveSucceeded(summary("stale", true)),
        ),
        SettingsCommandOutcome::Ignored,
    );
    assert_eq!(state.provider_models.pending_secret_len, 7);
    let mut saved = summary("codex", true);
    saved.default_model = Some("model-b".into());
    assert_eq!(
        reduce_settings_command(&mut state, AppCommand::ProviderSaveSucceeded(saved.clone()),),
        SettingsCommandOutcome::Applied,
    );
    assert_eq!(
        state.provider_models.status,
        ProviderModelsStatus::Saved {
            provider_id: "codex".into()
        }
    );
    assert!(state.provider_models.credential_configured);
    assert_eq!(state.provider_models.pending_secret_len, 0);
    assert_eq!(
        state.provider_models.editor,
        Some(ProviderDraft::from(&saved))
    );
    assert_eq!(state.provider_models.load, LoadState::Ready(vec![saved]));
}

#[test]
fn save_failure_keeps_the_safe_draft_available_for_retry_or_cancel() {
    let mut state = demo_state();
    reduce_settings_command(
        &mut state,
        AppCommand::BeginProviderEdit {
            draft: draft("codex"),
            credential_configured: true,
        },
    );
    reduce_settings_command(&mut state, AppCommand::RequestSaveProvider);

    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::ProviderSaveFailed {
                provider_id: "other".into(),
                message: "stale".into(),
            },
        ),
        SettingsCommandOutcome::Ignored,
    );
    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::ProviderSaveFailed {
                provider_id: "codex".into(),
                message: "keychain unavailable".into(),
            },
        ),
        SettingsCommandOutcome::Applied,
    );
    assert!(state.provider_models.editor.is_some());
    assert!(state.provider_models.credential_configured);
    assert!(matches!(
        state.provider_models.status,
        ProviderModelsStatus::Failed { ref message, .. } if message == "keychain unavailable"
    ));
    // Simulate keystrokes typed before the retry; cancelling must clear them.
    state.provider_models.pending_secret_len = 5;
    assert_eq!(
        reduce_settings_command(&mut state, AppCommand::CancelProviderEdit),
        SettingsCommandOutcome::Applied,
    );
    assert!(state.provider_models.editor.is_none());
    assert!(!state.provider_models.credential_configured);
    assert_eq!(state.provider_models.pending_secret_len, 0);
}

#[test]
fn removal_requires_a_loaded_provider_and_ignores_stale_completions() {
    let mut state = demo_state();
    state.provider_models.load =
        LoadState::Ready(vec![summary("codex", true), summary("ollama", false)]);
    state.provider_models.editor = Some(draft("codex"));
    state.provider_models.credential_configured = true;
    state.provider_models.pending_secret_len = 4;

    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::RequestRemoveProvider {
                provider_id: "missing".into(),
            },
        ),
        SettingsCommandOutcome::Ignored,
    );
    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::RequestRemoveProvider {
                provider_id: "codex".into(),
            },
        ),
        SettingsCommandOutcome::Applied,
    );
    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::ProviderRemoveSucceeded {
                provider_id: "ollama".into(),
            },
        ),
        SettingsCommandOutcome::Ignored,
    );
    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::ProviderRemoveFailed {
                provider_id: "codex".into(),
                message: "busy".into(),
            },
        ),
        SettingsCommandOutcome::Applied,
    );
    assert!(state.provider_models.editor.is_some());

    reduce_settings_command(
        &mut state,
        AppCommand::RequestRemoveProvider {
            provider_id: "codex".into(),
        },
    );
    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::ProviderRemoveSucceeded {
                provider_id: "codex".into(),
            },
        ),
        SettingsCommandOutcome::Applied,
    );
    let LoadState::Ready(providers) = &state.provider_models.load else {
        panic!("providers should stay loaded");
    };
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].provider_id, "ollama");
    assert!(state.provider_models.editor.is_none());
    assert!(!state.provider_models.credential_configured);
    assert_eq!(state.provider_models.pending_secret_len, 0);
    assert_eq!(
        state.provider_models.status,
        ProviderModelsStatus::Removed {
            provider_id: "codex".into()
        }
    );
}
