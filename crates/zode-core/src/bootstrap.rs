use std::path::{Path, PathBuf};

use crate::approval::{approval_queue, ApprovalReceiver};
use crate::config::{ConfigManager, ZodeConfig};
use crate::question::{question_queue, QuestionQueue, QuestionReceiver};
use crate::sandbox::{SandboxConfig, SandboxMode};
use crate::{CoreError, EngineTemplate};

/// Session-only launch overrides shared by the CLI and embedded app runtime.
#[derive(Debug, Clone, Default)]
pub struct BootstrapOverrides {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub yolo: bool,
    pub sandbox_enabled: Option<bool>,
    pub sandbox_read_only: bool,
    pub sandbox_allow_network: bool,
    pub sandbox_strict_read: bool,
    pub browser_enabled: Option<bool>,
}

/// Everything an interactive frontend needs to assemble and drive engines.
pub struct ResolvedBootstrap {
    pub needs_setup: bool,
    pub cfg: ZodeConfig,
    pub sandbox: Option<SandboxConfig>,
    pub template: EngineTemplate,
    pub approval_rx: ApprovalReceiver,
    pub question_rx: QuestionReceiver,
    pub question_queue: QuestionQueue,
}

/// Resolve one interactive launch without relying on process-global state.
pub struct AppBootstrap {
    cwd: PathBuf,
    config_dir: Option<PathBuf>,
    overrides: BootstrapOverrides,
    test_mode: bool,
    date: Option<String>,
}

impl AppBootstrap {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            config_dir: None,
            overrides: BootstrapOverrides::default(),
            test_mode: false,
            date: None,
        }
    }

    /// Deterministic bootstrap for tests and embedded runtime contract tests.
    /// Host credentials, browser processes, and sandbox backend discovery are
    /// disabled unless the corresponding override explicitly opts in.
    pub fn for_test(config_dir: impl Into<PathBuf>) -> Self {
        let config_dir = config_dir.into();
        Self {
            cwd: config_dir.clone(),
            config_dir: Some(config_dir),
            overrides: BootstrapOverrides::default(),
            test_mode: true,
            date: Some("1970-01-01".to_string()),
        }
    }

    pub fn with_config_dir(mut self, config_dir: impl Into<PathBuf>) -> Self {
        self.config_dir = Some(config_dir.into());
        self
    }

    pub fn with_overrides(mut self, overrides: BootstrapOverrides) -> Self {
        self.overrides = overrides;
        self
    }

    pub fn with_date(mut self, date: String) -> Self {
        self.date = Some(date);
        self
    }

    pub async fn resolve(self) -> Result<ResolvedBootstrap, CoreError> {
        let config_dir = match self.config_dir {
            Some(dir) => dir,
            None => ConfigManager::config_dir()?,
        };
        if let Err(error) = ConfigManager::ensure_default_global_in(&config_dir) {
            // Starter config creation has always been best-effort: a read-only
            // home must not prevent the UI from opening.
            tracing::warn!(%error, "could not write starter config");
        }

        let mut cfg = ConfigManager::load_in(&self.cwd, &config_dir, !self.test_mode)?;
        apply_provider_overrides(&mut cfg, &self.overrides, !self.test_mode)?;

        if let Some(enabled) = self.overrides.browser_enabled {
            cfg.browser.enabled = Some(enabled);
        } else if self.test_mode {
            cfg.browser.enabled = Some(false);
        }

        let sandbox = resolve_sandbox(&self.cwd, &cfg, &self.overrides, self.test_mode)?;
        let needs_setup = cfg.prepare_for_interactive_launch();

        let (queue, approval_rx) = approval_queue();
        let (question_queue, question_rx) = question_queue();
        let date = self.date.unwrap_or_else(today_utc);
        let template = EngineTemplate::new(
            cfg.clone(),
            self.cwd,
            Some(queue),
            self.overrides.yolo,
            sandbox.clone(),
            date,
        )
        .with_question_queue(Some(question_queue.clone()));

        Ok(ResolvedBootstrap {
            needs_setup,
            cfg,
            sandbox,
            template,
            approval_rx,
            question_rx,
            question_queue,
        })
    }
}

fn today_utc() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400;
    let (year, month, day) = civil_date_from_unix_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

// Gregorian calendar conversion for UTC days since 1970-01-01. Keeping this
// dependency-free lets zode-core own bootstrap date resolution while the CLI
// remains free to inject a fixed date through `with_date`.
fn civil_date_from_unix_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

fn apply_provider_overrides(
    cfg: &mut ZodeConfig,
    overrides: &BootstrapOverrides,
    apply_env_fallbacks: bool,
) -> Result<(), CoreError> {
    match (&overrides.provider, &overrides.model) {
        (Some(name), Some(model)) => {
            cfg.provider = cfg
                .resolve_named_provider_model(name, model)
                .ok_or_else(|| {
                    CoreError::Other(format!("no provider named '{name}' in config.providers"))
                })?;
        }
        (Some(name), None) => {
            cfg.provider = cfg.resolve_named_provider(name).ok_or_else(|| {
                CoreError::Other(format!("no provider named '{name}' in config.providers"))
            })?;
        }
        (None, Some(model)) => cfg.provider.model = Some(model.clone()),
        (None, None) => {}
    }
    cfg.resolve_provider_from_map();
    if apply_env_fallbacks {
        cfg.apply_env_fallbacks();
    }
    Ok(())
}

fn resolve_sandbox(
    cwd: &Path,
    cfg: &ZodeConfig,
    overrides: &BootstrapOverrides,
    test_mode: bool,
) -> Result<Option<SandboxConfig>, CoreError> {
    let enabled = overrides
        .sandbox_enabled
        .unwrap_or_else(|| !test_mode && cfg.sandbox.enabled.unwrap_or(true));
    if !enabled {
        return Ok(None);
    }

    let mode = if overrides.sandbox_read_only {
        SandboxMode::ReadOnly
    } else {
        cfg.sandbox
            .mode
            .as_deref()
            .map(SandboxMode::parse)
            .unwrap_or_default()
    };
    let allow_network = overrides.sandbox_allow_network || cfg.sandbox.network.unwrap_or(false);
    let roots: Vec<PathBuf> = cfg
        .sandbox
        .writable_roots
        .iter()
        .map(PathBuf::from)
        .collect();
    let exclude_slash_tmp = cfg.sandbox.exclude_slash_tmp.unwrap_or(false);
    let exclude_tmpdir_env_var = cfg.sandbox.exclude_tmpdir_env_var.unwrap_or(false);
    let restrict_reads =
        overrides.sandbox_strict_read || cfg.sandbox.restrict_reads.unwrap_or(false);

    if test_mode {
        return SandboxConfig::new(cwd, mode, allow_network, &roots)
            .map(|sandbox| {
                sandbox
                    .with_temp_policy(exclude_slash_tmp, exclude_tmpdir_env_var)
                    .with_restrict_reads(restrict_reads)
            })
            .map(Some);
    }

    crate::sandbox::resolve(
        cwd,
        true,
        mode,
        allow_network,
        &roots,
        exclude_slash_tmp,
        exclude_tmpdir_env_var,
    )
    .map(|sandbox| sandbox.map(|sandbox| sandbox.with_restrict_reads(restrict_reads)))
}

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
