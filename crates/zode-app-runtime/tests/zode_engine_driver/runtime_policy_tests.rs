#[tokio::test]
async fn provider_reload_falls_back_when_a_loaded_session_model_was_removed() {
    let dir = TestDir::new("provider-reload-removed-model");
    let config_dir = dir.path().join("config");
    let project = dir.path().join("project");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&project).unwrap();
    let node_id = NodeId::new();
    let session = session(node_id, "provider-reload-removed-model");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let first = Arc::new(FakeSessionEngine::new(Vec::new()));
    let replacement = Arc::new(FakeSessionEngine::new(Vec::new()));
    let factory = Arc::new(FakeFactory::new(vec![first, replacement]));
    let driver = ZodeEngineDriver::with_factory_and_config_dir(
        node_id,
        template(dir.path(), "removed-model"),
        repository.clone(),
        manifest(node_id),
        factory.clone(),
        config_dir.clone(),
    );
    driver
        .command(create_command(
            session.clone(),
            workspace(&project),
            "removed-model",
        ))
        .await
        .unwrap();
    let turn_id = TurnId::new();
    let events = driver
        .start_turn(
            start_command(session.clone(), turn_id, "load removed model session"),
            AbortController::new(),
        )
        .await;
    collect_stream(events).await;
    driver
        .finish_turn(&session, turn_id, None, false)
        .await
        .unwrap();

    let mut config = ZodeConfig::default();
    config.provider.model = Some("replacement-model".into());
    config.providers.insert(
        "replacement".into(),
        ProviderConfig {
            r#type: Some(ProviderKind::Openai),
            api_key: Some("replacement-key".into()),
            model: Some("replacement-model".into()),
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
        assert_eq!(assemblies[1].model, "replacement-model");
        assert_eq!(
            assemblies[1].template_model.as_deref(),
            Some("replacement-model")
        );
        assert_eq!(
            assemblies[1].active_provider_name.as_deref(),
            Some("replacement")
        );
    }

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
        Some("replacement-model")
    );

    let loaded = repository.load(&session).await.unwrap();
    assert_eq!(loaded.meta.model, "replacement-model");
}

#[tokio::test]
async fn provider_reload_normalizes_legacy_global_model() {
    let dir = TestDir::new("provider-reload-legacy");
    let config_dir = dir.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.json"),
        r#"{
            "provider": {
                "type": "openai",
                "apiKey": "legacy-test-key"
            },
            "model": "legacy-global-model"
        }"#,
    )
    .unwrap();
    let node_id = NodeId::new();
    let command_session = session(node_id, "provider-reload-legacy");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let factory = Arc::new(FakeFactory::new(Vec::new()));
    let driver = ZodeEngineDriver::with_factory_and_config_dir(
        node_id,
        template(dir.path(), "launch-model"),
        repository,
        manifest(node_id),
        factory,
        config_dir,
    );

    driver
        .command(command(
            command_session,
            None,
            AgentCommandKind::ReloadProviderConfiguration,
        ))
        .await
        .unwrap();

    let AgentSnapshot::RuntimeOptions(options) =
        driver.query(AgentQuery::RuntimeOptions).await.unwrap()
    else {
        panic!("expected runtime options");
    };
    assert_eq!(options.active_model.as_deref(), Some("legacy-global-model"));
}

#[tokio::test]
async fn revoking_project_permission_reassembles_loaded_session_gate() {
    let dir = TestDir::new("revoke-permission");
    let node_id = NodeId::new();
    let session = session(node_id, "permission-id");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let first = Arc::new(FakeSessionEngine::new(Vec::new()));
    let replacement = Arc::new(FakeSessionEngine::new(Vec::new()));
    let factory = Arc::new(FakeFactory::new(vec![first, replacement]));
    let driver = ZodeEngineDriver::with_factory(
        node_id,
        template(dir.path(), "permission-model"),
        repository,
        manifest(node_id),
        factory.clone(),
    );
    let project = dir.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let workspace_uri = workspace(&project);
    driver
        .command(create_command(
            session.clone(),
            workspace_uri.clone(),
            "permission-model",
        ))
        .await
        .unwrap();
    let turn_id = TurnId::new();
    let events = driver
        .start_turn(
            start_command(session.clone(), turn_id, "load gate"),
            AbortController::new(),
        )
        .await;
    collect_stream(events).await;
    driver
        .finish_turn(&session, turn_id, None, false)
        .await
        .unwrap();
    zode_core::config::ConfigManager::allow_project_tool(&project, "Bash").unwrap();

    driver
        .command(command(
            session,
            None,
            AgentCommandKind::RevokeProjectPermission {
                workspace_uri,
                tool: "Bash".into(),
            },
        ))
        .await
        .unwrap();

    assert!(
        zode_core::config::ConfigManager::project_allowed_tools(&project)
            .unwrap()
            .is_empty()
    );
    let assemblies = factory.assemblies.lock().unwrap();
    assert_eq!(assemblies.len(), 2, "gate topology must be rebuilt");
    assert!(assemblies[1].carried);
    assert_eq!(assemblies[1].prior_messages, 2);
}

#[tokio::test]
async fn steer_and_all_query_shapes_delegate_to_stable_sources() {
    let dir = TestDir::new("queries");
    let node_id = NodeId::new();
    let session = session(node_id, "query-id");
    let capabilities = manifest(node_id);
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let engine = Arc::new(FakeSessionEngine::new(Vec::new()));
    let factory = Arc::new(FakeFactory::new(vec![engine.clone()]));
    let driver = ZodeEngineDriver::with_factory(
        node_id,
        template(dir.path(), "query-model"),
        repository,
        capabilities.clone(),
        factory,
    );
    let project = dir.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let workspace_uri = workspace(&project);
    driver
        .command(create_command(
            session.clone(),
            workspace_uri.clone(),
            "query-model",
        ))
        .await
        .unwrap();
    let turn_id = TurnId::new();
    let _events = driver
        .start_turn(
            start_command(session.clone(), turn_id, "query state"),
            AbortController::new(),
        )
        .await;
    driver
        .command(command(
            session.clone(),
            Some(turn_id),
            AgentCommandKind::SteerTurn {
                input: vec![UserContent::Text {
                    text: "steer now".into(),
                }],
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        &engine.steered.lock().unwrap()[0][0],
        ContentBlock::Text { text } if text == "steer now"
    ));

    assert_eq!(
        driver.query(AgentQuery::Capabilities).await.unwrap(),
        AgentSnapshot::Capabilities(capabilities)
    );
    let AgentSnapshot::Threads(threads) = driver.query(AgentQuery::Threads).await.unwrap() else {
        panic!("expected thread snapshot");
    };
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].session, session);
    let AgentSnapshot::RuntimeOptions(options) =
        driver.query(AgentQuery::RuntimeOptions).await.unwrap()
    else {
        panic!("expected runtime options");
    };
    assert_eq!(options.active_model.as_deref(), Some("query-model"));
    assert!(options.models.contains(&"query-model".to_string()));
    let AgentSnapshot::Diff(DiffSnapshot {
        session: diff_session,
        files,
        unified,
    }) = driver
        .query(AgentQuery::Diff {
            session: session.clone(),
        })
        .await
        .unwrap()
    else {
        panic!("expected diff snapshot");
    };
    assert_eq!(diff_session, session);
    assert!(files.is_empty());
    assert!(unified.is_empty());
    let AgentSnapshot::Integrations(integrations) = driver
        .query(AgentQuery::Integrations {
            workspace_uri: workspace_uri.clone(),
        })
        .await
        .unwrap()
    else {
        panic!("expected integrations snapshot");
    };
    assert_eq!(integrations.workspace_uri, workspace_uri);
    assert!(integrations.entries.len() >= 10);
    assert!(integrations
        .entries
        .iter()
        .all(|entry| !entry.source_id.is_empty()));
    assert!(integrations.entries.iter().all(|entry| {
        entry.kind != zode_node_protocol::IntegrationRegistryKind::Mcp
            || matches!(
                entry.state,
                zode_node_protocol::IntegrationRegistryState::Configured
                    | zode_node_protocol::IntegrationRegistryState::Disabled
            )
    }));
    assert_eq!(
        driver
            .query(AgentQuery::ProjectPermissions { workspace_uri })
            .await
            .unwrap(),
        AgentSnapshot::ProjectPermissions(Vec::new())
    );
}

#[tokio::test]
async fn integration_discovery_never_assembles_a_session_engine() {
    let dir = TestDir::new("integration-query");
    let project = dir.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let node_id = NodeId::new();
    let capabilities = manifest(node_id);
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let factory = Arc::new(FakeFactory::new(Vec::new()));
    let driver = ZodeEngineDriver::with_factory_and_config_dir(
        node_id,
        template(&project, "query-model"),
        repository,
        capabilities,
        factory.clone(),
        dir.path().join("config"),
    );
    let workspace_uri = workspace(&project);

    let AgentSnapshot::Integrations(snapshot) = driver
        .query(AgentQuery::Integrations {
            workspace_uri: workspace_uri.clone(),
        })
        .await
        .unwrap()
    else {
        panic!("expected integrations snapshot");
    };

    assert_eq!(snapshot.workspace_uri, workspace_uri);
    assert!(snapshot.entries.len() >= 10);
    assert!(factory.assemblies.lock().unwrap().is_empty());
}

#[tokio::test]
async fn session_runtime_options_read_back_only_the_addressed_loaded_session() {
    let dir = TestDir::new("session-options");
    let node_id = NodeId::new();
    let session_a = session(node_id, "session-a");
    let session_b = session(node_id, "session-b");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let engines = (0..5)
        .map(|_| Arc::new(FakeSessionEngine::new(Vec::new())))
        .collect();
    let factory = Arc::new(FakeFactory::new(engines));
    let driver = ZodeEngineDriver::with_factory(
        node_id,
        template(dir.path(), "initial-model"),
        repository,
        manifest(node_id),
        factory,
    );
    for (session, project) in [
        (session_a.clone(), dir.path().join("project-a")),
        (session_b.clone(), dir.path().join("project-b")),
    ] {
        fs::create_dir_all(&project).unwrap();
        driver
            .command(create_command(
                session.clone(),
                workspace(&project),
                "initial-model",
            ))
            .await
            .unwrap();
        let _ = driver
            .start_turn(
                start_command(session, TurnId::new(), "load"),
                AbortController::new(),
            )
            .await;
    }

    driver
        .command(command(
            session_a.clone(),
            None,
            AgentCommandKind::SetPermissionPreset {
                approval_mode: ApprovalMode::Auto,
                sandbox_mode: SandboxMode::ReadOnly,
                network: true,
            },
        ))
        .await
        .unwrap();
    driver
        .command(command(
            session_a.clone(),
            None,
            AgentCommandKind::SetModel {
                model: "session-a-model".into(),
            },
        ))
        .await
        .unwrap();
    driver
        .command(command(
            session_a.clone(),
            None,
            AgentCommandKind::SetEffort {
                effort: "high".into(),
            },
        ))
        .await
        .unwrap();

    let options_a = assert_runtime_options(
        driver
            .query(AgentQuery::SessionRuntimeOptions {
                session: session_a.clone(),
            })
            .await
            .unwrap(),
        &session_a,
    );
    assert_eq!(options_a.active_model.as_deref(), Some("session-a-model"));
    assert_eq!(options_a.effort.as_deref(), Some("high"));
    assert_eq!(options_a.approval_mode, ApprovalMode::Auto);
    assert_eq!(options_a.sandbox_mode, SandboxMode::ReadOnly);
    assert!(options_a.sandbox_network);

    let options_b = assert_runtime_options(
        driver
            .query(AgentQuery::SessionRuntimeOptions {
                session: session_b.clone(),
            })
            .await
            .unwrap(),
        &session_b,
    );
    assert_eq!(options_b.active_model.as_deref(), Some("initial-model"));
    assert_eq!(options_b.effort, None);
    assert_eq!(options_b.approval_mode, ApprovalMode::Request);
    assert_eq!(options_b.sandbox_mode, SandboxMode::Off);
    assert!(!options_b.sandbox_network);
}

#[tokio::test]
async fn unloaded_session_runtime_options_use_its_persisted_workspace_policy_without_assembly() {
    let dir = TestDir::new("unloaded-options");
    let config_dir = dir.path().join("config");
    let project = dir.path().join("project");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&project).unwrap();
    zode_core::config::ConfigManager::update_project_state(&project, |state| {
        state.insert(
            "sandbox".into(),
            serde_json::json!({
                "enabled": true,
                "mode": "read-only",
                "network": true
            }),
        );
    })
    .unwrap();

    let node_id = NodeId::new();
    let session = session(node_id, "unloaded");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    repository
        .create(&session, &workspace(&project), "session-model".into())
        .await
        .unwrap();
    let factory = Arc::new(FakeFactory::new(Vec::new()));
    let driver = ZodeEngineDriver::with_factory_and_config_dir(
        node_id,
        template(dir.path(), "launch-model"),
        repository,
        manifest(node_id),
        factory.clone(),
        config_dir,
    );

    let options = assert_runtime_options(
        driver
            .query(AgentQuery::SessionRuntimeOptions {
                session: session.clone(),
            })
            .await
            .unwrap(),
        &session,
    );
    assert_eq!(options.active_model.as_deref(), Some("session-model"));
    assert_eq!(options.sandbox_mode, SandboxMode::ReadOnly);
    assert!(options.sandbox_network);
    assert!(factory.assemblies.lock().unwrap().is_empty());
}

#[tokio::test]
async fn engine_assembly_isolates_sandbox_and_permission_policy_per_workspace() {
    let dir = TestDir::new("workspace-policy");
    let config_dir = dir.path().join("config");
    let project_a = dir.path().join("project-a");
    let project_b = dir.path().join("project-b");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&project_a).unwrap();
    fs::create_dir_all(&project_b).unwrap();
    zode_core::config::ConfigManager::update_project_state(&project_a, |state| {
        state.insert(
            "sandbox".into(),
            serde_json::json!({
                "enabled": true,
                "mode": "read-only",
                "network": true
            }),
        );
        state.insert("permissions".into(), serde_json::json!({"allow": ["Bash"]}));
    })
    .unwrap();
    zode_core::config::ConfigManager::update_project_state(&project_b, |state| {
        state.insert("sandbox".into(), serde_json::json!({"enabled": false}));
        state.insert(
            "permissions".into(),
            serde_json::json!({"allow": ["FileWrite"]}),
        );
    })
    .unwrap();

    let node_id = NodeId::new();
    let session_a = session(node_id, "workspace-a");
    let session_b = session(node_id, "workspace-b");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    for (session, project) in [(&session_a, &project_a), (&session_b, &project_b)] {
        repository
            .create(session, &workspace(project), "shared-model".into())
            .await
            .unwrap();
    }
    let factory = Arc::new(FakeFactory::new(vec![
        Arc::new(FakeSessionEngine::new(Vec::new())),
        Arc::new(FakeSessionEngine::new(Vec::new())),
    ]));
    let driver = ZodeEngineDriver::with_factory_and_config_dir(
        node_id,
        template(dir.path(), "launch-model"),
        repository,
        manifest(node_id),
        factory.clone(),
        config_dir,
    );

    for session in [session_a, session_b] {
        let _ = driver
            .start_turn(
                start_command(session, TurnId::new(), "load"),
                AbortController::new(),
            )
            .await;
    }

    let assemblies = factory.assemblies.lock().unwrap();
    assert_eq!(assemblies[0].sandbox_mode, SandboxMode::ReadOnly);
    assert!(assemblies[0].sandbox_network);
    assert_eq!(assemblies[0].allowed_tools, vec!["Bash"]);
    assert_eq!(assemblies[1].sandbox_mode, SandboxMode::Off);
    assert!(!assemblies[1].sandbox_network);
    assert_eq!(assemblies[1].allowed_tools, vec!["FileWrite"]);
}

#[tokio::test]
async fn failed_sandbox_reassembly_keeps_live_and_persisted_policy_unchanged() {
    let dir = TestDir::new("sandbox-transaction");
    let project = dir.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let node_id = NodeId::new();
    let session = session(node_id, "sandbox-transaction");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let factory = Arc::new(FailOnSecondAssemblyFactory {
        first: Arc::new(FakeSessionEngine::new(Vec::new())),
        calls: AtomicUsize::new(0),
    });
    let driver = ZodeEngineDriver::with_factory(
        node_id,
        template(dir.path(), "model"),
        repository,
        manifest(node_id),
        factory,
    );
    driver
        .command(create_command(
            session.clone(),
            workspace(&project),
            "model",
        ))
        .await
        .unwrap();
    let _ = driver
        .start_turn(
            start_command(session.clone(), TurnId::new(), "load"),
            AbortController::new(),
        )
        .await;

    let error = driver
        .command(command(
            session.clone(),
            None,
            AgentCommandKind::SetSandbox {
                mode: SandboxMode::ReadOnly,
                network: true,
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(error.kind, EndpointErrorKind::Internal);
    assert!(!zode_core::config::ConfigManager::project_state_path(&project).exists());

    let options = assert_runtime_options(
        driver
            .query(AgentQuery::SessionRuntimeOptions {
                session: session.clone(),
            })
            .await
            .unwrap(),
        &session,
    );
    assert_eq!(options.sandbox_mode, SandboxMode::Off);
    assert!(!options.sandbox_network);
}
