#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::{json, Value};

    use super::{AppBootstrap, BootstrapOverrides, ResolvedBootstrap};
    use crate::config::DEFAULT_STARTER_MODEL;
    use crate::sandbox::SandboxMode;

    fn write_config(dir: &Path, config: Value) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("config.json"),
            serde_json::to_vec_pretty(&config).unwrap(),
        )
        .unwrap();
    }

    fn assert_interactive_handles(resolved: &ResolvedBootstrap) {
        let _: &crate::EngineTemplate = &resolved.template;
        let _: &crate::approval::ApprovalReceiver = &resolved.approval_rx;
        let _: &crate::question::QuestionReceiver = &resolved.question_rx;
        let _: &crate::question::QuestionQueue = &resolved.question_queue;
    }

    #[tokio::test]
    async fn interactive_bootstrap_prepares_missing_provider_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            json!({
                "sandbox": { "enabled": false },
                "browser": { "enabled": false }
            }),
        );

        let resolved = AppBootstrap::for_test(dir.path())
            .resolve()
            .await
            .expect("interactive setup must remain launchable without credentials");

        assert!(resolved.needs_setup);
        assert_eq!(
            resolved.cfg.provider.model.as_deref(),
            Some(DEFAULT_STARTER_MODEL)
        );
        assert_eq!(resolved.cfg.provider.api_key.as_deref(), Some(""));
        assert_interactive_handles(&resolved);
    }

    #[tokio::test]
    async fn explicit_config_dir_wins_without_changing_process_environment() {
        let cwd = tempfile::tempdir().unwrap();
        let config_dir = tempfile::tempdir().unwrap();
        write_config(
            config_dir.path(),
            json!({
                "provider": {
                    "type": "ollama",
                    "model": "explicit-config-model",
                    "baseUrl": "http://127.0.0.1:11434"
                },
                "sandbox": { "enabled": false },
                "browser": { "enabled": false }
            }),
        );
        let config_env_before = std::env::var_os("ZODE_CONFIG_DIR");

        let resolved = AppBootstrap::new(cwd.path())
            .with_config_dir(config_dir.path())
            .with_overrides(BootstrapOverrides {
                sandbox_enabled: Some(false),
                browser_enabled: Some(false),
                ..BootstrapOverrides::default()
            })
            .resolve()
            .await
            .unwrap();

        assert_eq!(
            resolved.cfg.provider.model.as_deref(),
            Some("explicit-config-model")
        );
        assert_eq!(std::env::var_os("ZODE_CONFIG_DIR"), config_env_before);
    }

    #[tokio::test]
    async fn provider_and_model_overrides_are_resolved_as_one_pair() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            json!({
                "providers": {
                    "custom": {
                        "type": "openai",
                        "apiKey": "test-key",
                        "baseUrl": "http://provider.invalid/v1",
                        "model": "default-model",
                        "models": {
                            "default-model": { "contextWindow": 4096 },
                            "chosen-model": {
                                "contextWindow": 98765,
                                "maxOutputTokens": 4321
                            }
                        }
                    }
                },
                "sandbox": { "enabled": false },
                "browser": { "enabled": false }
            }),
        );

        let resolved = AppBootstrap::for_test(dir.path())
            .with_overrides(BootstrapOverrides {
                provider: Some("custom".into()),
                model: Some("chosen-model".into()),
                ..BootstrapOverrides::default()
            })
            .resolve()
            .await
            .unwrap();

        assert!(!resolved.needs_setup);
        assert_eq!(resolved.cfg.provider.model.as_deref(), Some("chosen-model"));
        assert_eq!(resolved.cfg.provider.context_window, Some(98765));
        assert_eq!(resolved.cfg.provider.max_output_tokens, Some(4321));
        assert_eq!(
            resolved.cfg.provider.base_url.as_deref(),
            Some("http://provider.invalid/v1")
        );
    }

    #[tokio::test]
    async fn sandbox_and_browser_overrides_take_priority_over_config() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            json!({
                "provider": {
                    "type": "ollama",
                    "model": "test-model",
                    "baseUrl": "http://127.0.0.1:11434"
                },
                "sandbox": {
                    "enabled": false,
                    "mode": "workspace-write",
                    "network": false,
                    "restrictReads": false
                },
                "browser": { "enabled": true }
            }),
        );

        let resolved = AppBootstrap::for_test(dir.path())
            .with_overrides(BootstrapOverrides {
                provider: None,
                model: None,
                yolo: true,
                sandbox_enabled: Some(true),
                sandbox_read_only: true,
                sandbox_allow_network: true,
                sandbox_strict_read: true,
                browser_enabled: Some(false),
            })
            .resolve()
            .await
            .unwrap();

        let sandbox = resolved.sandbox.expect("override should enable sandbox");
        assert_eq!(sandbox.mode(), SandboxMode::ReadOnly);
        assert!(sandbox.allow_network());
        assert!(sandbox.restrict_reads());
        assert!(!resolved.cfg.browser.enabled());
    }

    #[tokio::test]
    async fn for_test_ignores_host_credentials_and_sandbox_backend() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            json!({
                "provider": { "type": "anthropic" },
                "sandbox": { "enabled": true },
                "browser": { "enabled": true }
            }),
        );

        let resolved = AppBootstrap::for_test(dir.path())
            .resolve()
            .await
            .expect("test bootstrap must not require host credentials or bwrap");

        assert!(resolved.needs_setup);
        assert_eq!(resolved.cfg.provider.api_key.as_deref(), Some(""));
        assert!(resolved.sandbox.is_none());
        assert!(!resolved.cfg.browser.enabled());
    }
}
