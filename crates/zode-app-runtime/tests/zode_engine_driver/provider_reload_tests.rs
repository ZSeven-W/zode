#[tokio::test]
async fn create_is_lazy_and_first_turn_persists_snapshot_model_and_title() {
    let dir = TestDir::new("turn-persist");
    let node_id = NodeId::new();
    let session = session(node_id, "caller-id");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let engine = Arc::new(FakeSessionEngine::new(vec![
        Ok(Event::TextDelta {
            delta: "reply".into(),
        }),
        Ok(Event::Usage {
            input_tokens: 10,
            output_tokens: 3,
            cache_read: 0,
            cache_create: 0,
        }),
        Ok(Event::Result {
            data: agent::stream::ResultData {
                stop_reason: Some("end_turn".into()),
                model: Some("resolved-model".into()),
                metadata: Default::default(),
            },
        }),
    ]));
    let factory = Arc::new(FakeFactory::new(vec![engine.clone()]));
    let driver = ZodeEngineDriver::with_factory(
        node_id,
        template(dir.path(), "initial-model"),
        repository.clone(),
        manifest(node_id),
        factory.clone(),
    );
    let project = dir.path().join("project");

    driver
        .command(create_command(
            session.clone(),
            workspace(&project),
            "initial-model",
        ))
        .await
        .unwrap();
    assert!(factory.assemblies.lock().unwrap().is_empty());
    assert_eq!(
        repository.load(&session).await.unwrap().meta.id,
        "caller-id"
    );

    let turn_id = TurnId::new();
    let events = driver
        .start_turn(
            start_command(session.clone(), turn_id, "Design the desktop shell"),
            AbortController::new(),
        )
        .await;
    assert_eq!(factory.assemblies.lock().unwrap().len(), 1);
    {
        let started = engine.started.lock().unwrap();
        assert!(
            matches!(&started[0][0], ContentBlock::Text { text } if text == "Design the desktop shell")
        );
        assert!(matches!(
            &started[0][1],
            ContentBlock::Image { source: ImageSource::Base64 { media_type, data } }
                if media_type == "image/png" && data == "aGVsbG8="
        ));
    }

    let raw = collect_stream(events).await;
    let usage_event = raw
        .iter()
        .find(|event| matches!(event, Event::Usage { .. }))
        .unwrap();
    let cumulative = driver
        .observe_event(&session, turn_id, usage_event)
        .await
        .unwrap();
    assert_eq!(cumulative, engine.cumulative_usage);
    driver.finish_turn_usage(&session, turn_id);
    assert_eq!(engine.finish_usage_calls.load(Ordering::SeqCst), 1);

    driver
        .finish_turn(&session, turn_id, Some("resolved-model".into()), false)
        .await
        .unwrap();
    let loaded = repository.load(&session).await.unwrap();
    assert_eq!(loaded.meta.title, "Design the desktop shell");
    assert_eq!(loaded.meta.model, "resolved-model");
    assert_eq!(loaded.meta.cwd, project.to_string_lossy());
    assert_eq!(loaded.store.len(), 2);
}

#[tokio::test]
async fn restart_lazily_restores_the_persisted_transcript() {
    let dir = TestDir::new("restart");
    let node_id = NodeId::new();
    let session = session(node_id, "restart-id");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let loaded = repository
        .create(
            &session,
            &workspace(&dir.path().join("project")),
            "restored-model".into(),
        )
        .await
        .unwrap();
    let _saved = repository
        .save(
            &session,
            loaded.meta,
            store_with_exchange("persisted prompt"),
            SessionWriteMode::Full,
        )
        .await
        .unwrap();

    let engine = Arc::new(FakeSessionEngine::new(Vec::new()));
    let factory = Arc::new(FakeFactory::new(vec![engine]));
    let restarted = ZodeEngineDriver::with_factory(
        node_id,
        template(dir.path(), "fallback-model"),
        repository,
        manifest(node_id),
        factory.clone(),
    );

    let _events = restarted
        .start_turn(
            start_command(session.clone(), TurnId::new(), "continue"),
            AbortController::new(),
        )
        .await;
    let assemblies = factory.assemblies.lock().unwrap();
    assert_eq!(assemblies.len(), 1);
    assert_eq!(assemblies[0].session, session);
    assert_eq!(assemblies[0].prior_messages, 2);
    assert_eq!(assemblies[0].model, "restored-model");
    assert_eq!(
        assemblies[0].template_model.as_deref(),
        Some("restored-model")
    );
    assert!(!assemblies[0].carried);
}

#[tokio::test]
async fn idle_model_switch_reassembles_with_carry_and_preserves_transcript() {
    let dir = TestDir::new("model-switch");
    let node_id = NodeId::new();
    let session = session(node_id, "model-id");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let first = Arc::new(FakeSessionEngine::new(Vec::new()));
    let replacement = Arc::new(FakeSessionEngine::new(Vec::new()));
    let factory = Arc::new(FakeFactory::new(vec![first, replacement]));
    let driver = ZodeEngineDriver::with_factory(
        node_id,
        template(dir.path(), "old-model"),
        repository.clone(),
        manifest(node_id),
        factory.clone(),
    );
    driver
        .command(create_command(
            session.clone(),
            workspace(&dir.path().join("project")),
            "old-model",
        ))
        .await
        .unwrap();
    let turn_id = TurnId::new();
    let events = driver
        .start_turn(
            start_command(session.clone(), turn_id, "keep this transcript"),
            AbortController::new(),
        )
        .await;
    collect_stream(events).await;
    driver
        .finish_turn(&session, turn_id, None, false)
        .await
        .unwrap();

    driver
        .command(command(
            session.clone(),
            None,
            AgentCommandKind::SetModel {
                model: "new-model".into(),
            },
        ))
        .await
        .unwrap();

    {
        let assemblies = factory.assemblies.lock().unwrap();
        assert_eq!(assemblies.len(), 2);
        assert_eq!(assemblies[1].model, "new-model");
        assert_eq!(assemblies[1].template_model.as_deref(), Some("new-model"));
        assert_eq!(assemblies[1].prior_messages, 2);
        assert!(assemblies[1].carried);
    }
    let loaded = repository.load(&session).await.unwrap();
    assert_eq!(loaded.meta.model, "new-model");
    assert_eq!(loaded.store.len(), 2);
}

#[tokio::test]
async fn provider_reload_refreshes_base_and_loaded_sessions_without_losing_state() {
    let dir = TestDir::new("provider-reload");
    let config_dir = dir.path().join("config");
    let project = dir.path().join("project");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&project).unwrap();
    let node_id = NodeId::new();
    let session = session(node_id, "provider-reload");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let first = Arc::new(FakeSessionEngine::new(Vec::new()));
    let replacement = Arc::new(FakeSessionEngine::new(Vec::new()));
    let factory = Arc::new(FakeFactory::new(vec![first, replacement]));
    let driver = ZodeEngineDriver::with_factory_and_config_dir(
        node_id,
        template(dir.path(), "launch-model"),
        repository.clone(),
        manifest(node_id),
        factory.clone(),
        config_dir.clone(),
    );
    driver
        .command(create_command(
            session.clone(),
            workspace(&project),
            "session-model",
        ))
        .await
        .unwrap();
    let turn_id = TurnId::new();
    let events = driver
        .start_turn(
            start_command(session.clone(), turn_id, "keep provider transcript"),
            AbortController::new(),
        )
        .await;
    collect_stream(events).await;
    driver
        .finish_turn(&session, turn_id, None, false)
        .await
        .unwrap();

    let mut config = ZodeConfig::default();
    config.provider.model = Some("global-model".into());
    config.providers.insert(
        "global".into(),
        ProviderConfig {
            r#type: Some(ProviderKind::Openai),
            api_key: Some("updated-global-key".into()),
            model: Some("global-model".into()),
            ..Default::default()
        },
    );
    config.providers.insert(
        "session".into(),
        ProviderConfig {
            r#type: Some(ProviderKind::Anthropic),
            api_key: Some("updated-session-key".into()),
            model: Some("session-model".into()),
            ..Default::default()
        },
    );
    ConfigManager::save_global_in(&config_dir, &config).unwrap();

    driver
        .command(command(
            session.clone(),
            None,
            AgentCommandKind::ReloadProviderConfiguration,
        ))
        .await
        .unwrap();

    {
        let assemblies = factory.assemblies.lock().unwrap();
        assert_eq!(assemblies.len(), 2);
        assert_eq!(assemblies[1].model, "session-model");
        assert_eq!(
            assemblies[1].template_model.as_deref(),
            Some("session-model")
        );
        assert_eq!(assemblies[1].prior_messages, 2);
        assert!(assemblies[1].carried);
        assert_eq!(assemblies[1].provider_names, vec!["global", "session"]);
        assert_eq!(
            assemblies[1].active_provider_name.as_deref(),
            Some("session")
        );
    }

    let AgentSnapshot::RuntimeOptions(base_options) =
        driver.query(AgentQuery::RuntimeOptions).await.unwrap()
    else {
        panic!("expected base runtime options");
    };
    assert_eq!(base_options.active_model.as_deref(), Some("global-model"));
    assert!(base_options.models.contains(&"global-model".into()));
    assert!(base_options.models.contains(&"session-model".into()));

    let session_options = assert_runtime_options(
        driver
            .query(AgentQuery::SessionRuntimeOptions {
                session: session.clone(),
            })
            .await
            .unwrap(),
        &session,
    );
    assert_eq!(
        session_options.active_model.as_deref(),
        Some("session-model")
    );

    let loaded = repository.load(&session).await.unwrap();
    assert_eq!(loaded.meta.model, "session-model");
    assert_eq!(loaded.store.len(), 2);
}

#[tokio::test]
#[serial_test::serial]
async fn provider_reload_applies_env_fallback_to_non_active_session_provider() {
    let _anthropic_key = EnvVarGuard::set("ANTHROPIC_API_KEY", "session-env-key");
    let dir = TestDir::new("provider-reload-non-active-env");
    let config_dir = dir.path().join("config");
    let project = dir.path().join("project");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&project).unwrap();
    let node_id = NodeId::new();
    let session = session(node_id, "provider-reload-non-active-env");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let factory = Arc::new(ValidateProviderOnReloadFactory {
        inner: FakeFactory::new(vec![
            Arc::new(FakeSessionEngine::new(Vec::new())),
            Arc::new(FakeSessionEngine::new(Vec::new())),
        ]),
        calls: AtomicUsize::new(0),
    });
    let driver = ZodeEngineDriver::with_factory_and_config_dir(
        node_id,
        template(dir.path(), "session-model"),
        repository,
        manifest(node_id),
        factory.clone(),
        config_dir.clone(),
    );
    driver
        .command(create_command(
            session.clone(),
            workspace(&project),
            "session-model",
        ))
        .await
        .unwrap();
    let _ = driver
        .start_turn(
            start_command(session.clone(), TurnId::new(), "load env session"),
            AbortController::new(),
        )
        .await;

    let mut config = ZodeConfig::default();
    config.provider.model = Some("global-model".into());
    config.providers.insert(
        "global".into(),
        ProviderConfig {
            r#type: Some(ProviderKind::Openai),
            api_key: Some("global-explicit-key".into()),
            model: Some("global-model".into()),
            ..Default::default()
        },
    );
    config.providers.insert(
        "session".into(),
        ProviderConfig {
            r#type: Some(ProviderKind::Anthropic),
            model: Some("session-model".into()),
            ..Default::default()
        },
    );
    ConfigManager::save_global_in(&config_dir, &config).unwrap();

    driver
        .command(command(
            session.clone(),
            None,
            AgentCommandKind::ReloadProviderConfiguration,
        ))
        .await
        .unwrap();

    assert_eq!(factory.calls.load(Ordering::SeqCst), 2);
    let session_options = assert_runtime_options(
        driver
            .query(AgentQuery::SessionRuntimeOptions {
                session: session.clone(),
            })
            .await
            .unwrap(),
        &session,
    );
    assert_eq!(
        session_options.active_model.as_deref(),
        Some("session-model")
    );
}

#[tokio::test]
async fn failed_provider_reload_keeps_base_and_every_loaded_session_unchanged() {
    let dir = TestDir::new("provider-reload-transaction");
    let config_dir = dir.path().join("config");
    let project_a = dir.path().join("project-a");
    let project_b = dir.path().join("project-b");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&project_a).unwrap();
    fs::create_dir_all(&project_b).unwrap();
    let node_id = NodeId::new();
    let session_a = session(node_id, "provider-reload-transaction-a");
    let session_b = session(node_id, "provider-reload-transaction-b");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let initial_a = Arc::new(FakeSessionEngine::new(Vec::new()));
    let initial_b = Arc::new(FakeSessionEngine::new(Vec::new()));
    let staged_a = Arc::new(FakeSessionEngine::new(Vec::new()));
    let factory = Arc::new(FailAtAssemblyFactory {
        engines: Mutex::new(vec![initial_a.clone(), initial_b.clone(), staged_a.clone()].into()),
        calls: AtomicUsize::new(0),
        fail_at: 3,
    });
    let driver = ZodeEngineDriver::with_factory_and_config_dir(
        node_id,
        template(dir.path(), "launch-model"),
        repository.clone(),
        manifest(node_id),
        factory,
        config_dir.clone(),
    );
    for (session, project, model) in [
        (&session_a, &project_a, "old-model-a"),
        (&session_b, &project_b, "old-model-b"),
    ] {
        driver
            .command(create_command(session.clone(), workspace(project), model))
            .await
            .unwrap();
        let _ = driver
            .start_turn(
                start_command(session.clone(), TurnId::new(), "load old engine"),
                AbortController::new(),
            )
            .await;
    }

    let mut config = ZodeConfig::default();
    config.provider.model = Some("new-default-model".into());
    config.providers.insert(
        "replacement".into(),
        ProviderConfig {
            r#type: Some(ProviderKind::Openai),
            api_key: Some("replacement-key".into()),
            model: Some("new-default-model".into()),
            ..Default::default()
        },
    );
    ConfigManager::save_global_in(&config_dir, &config).unwrap();

    let error = driver
        .command(command(
            session_a.clone(),
            None,
            AgentCommandKind::ReloadProviderConfiguration,
        ))
        .await
        .unwrap_err();
    assert_eq!(error.kind, EndpointErrorKind::Internal);

    let AgentSnapshot::RuntimeOptions(base_options) =
        driver.query(AgentQuery::RuntimeOptions).await.unwrap()
    else {
        panic!("expected base runtime options");
    };
    assert_eq!(base_options.active_model.as_deref(), Some("launch-model"));
    for (session, model) in [(&session_a, "old-model-a"), (&session_b, "old-model-b")] {
        let options = assert_runtime_options(
            driver
                .query(AgentQuery::SessionRuntimeOptions {
                    session: session.clone(),
                })
                .await
                .unwrap(),
            session,
        );
        assert_eq!(options.active_model.as_deref(), Some(model));
        assert_eq!(repository.load(session).await.unwrap().meta.model, model);
    }

    for session in [&session_a, &session_b] {
        let _ = driver
            .start_turn(
                start_command(session.clone(), TurnId::new(), "still old engine"),
                AbortController::new(),
            )
            .await;
    }
    assert_eq!(initial_a.started.lock().unwrap().len(), 2);
    assert_eq!(initial_b.started.lock().unwrap().len(), 2);
    assert!(staged_a.started.lock().unwrap().is_empty());
}
