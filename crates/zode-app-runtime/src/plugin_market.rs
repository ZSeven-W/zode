//! Desktop bridge for the plugin marketplace: wraps `zode_core::plugin_market`
//! (installer, manifest, trust store) with the wire-safe types from
//! `zode-node-protocol`, mirroring how `crate::integrations` bridges the
//! existing skill/MCP/tool-group registry. Every function here is plain
//! config-dir-scoped I/O - the async ones (`install_plugin`) are safe to run
//! on the `CommandBridge`'s background tokio task exactly like every other
//! `AgentCommandKind`, so a slow git clone never blocks the UI thread.

use std::path::{Path, PathBuf};

use zode_core::config::ConfigManager;
use zode_core::plugin_market::installer::{self, plugins_root};
use zode_core::plugin_market::manifest::{Capability, PluginManifest};
use zode_core::plugin_market::trust::{TrustStatus, TrustStore};
use zode_core::plugin_market::update::{self, PendingUpdate, UpdateCheck};
use zode_core::plugin_market::PluginMarketError;
use zode_node_protocol::{
    InstalledPluginSummary, PluginCapabilityKind, PluginCapabilitySummary, PluginTrustItem,
    PluginTrustReview, PluginTrustState, PluginUpdateAvailable, PluginUpdateCheck,
};

pub(crate) async fn install_plugin(
    spec: &str,
    reference: Option<&str>,
    config_dir: Option<&Path>,
) -> Result<(), String> {
    let config_dir = resolve_config_dir(config_dir)?;
    installer::install(spec, reference, &config_dir)
        .await
        .map(|_| ())
        .map_err(plugin_market_error_message)
}

/// Fetches the plugin's pinned ref and reports whether the remote moved
/// ahead. Read-only - the worktree is only touched by [`apply_plugin_update`].
/// Slow (network I/O), so it runs on the same background task as the install.
pub(crate) async fn check_plugin_update(
    plugin_id: &str,
    config_dir: Option<&Path>,
) -> Result<PluginUpdateCheck, String> {
    let config_dir = resolve_config_dir(config_dir)?;
    let check = update::check_update(plugin_id, &config_dir)
        .await
        .map_err(plugin_market_error_message)?;
    Ok(PluginUpdateCheck {
        plugin_id: plugin_id.to_string(),
        available: match check {
            UpdateCheck::UpToDate { .. } => None,
            UpdateCheck::Available(pending) => Some(PluginUpdateAvailable {
                summary: update_summary(&pending),
                target_commit: pending.to,
            }),
        },
    })
}

/// Fast-forwards the plugin to its ref's current remote tip and re-scans its
/// capabilities. Trust records are deliberately left alone: the caller
/// refreshes the installed list right after, and the existing drift check
/// re-gates any capability whose executable content changed.
pub(crate) async fn apply_plugin_update(
    plugin_id: &str,
    config_dir: Option<&Path>,
) -> Result<(), String> {
    let config_dir = resolve_config_dir(config_dir)?;
    update::apply_update(plugin_id, &config_dir)
        .await
        .map(|_| ())
        .map_err(plugin_market_error_message)
}

/// zh-CN rendering of `PendingUpdate` - core keeps the structured fields and
/// an English fallback; the user-facing string is composed here alongside
/// every other localized plugin-market message.
fn update_summary(pending: &PendingUpdate) -> String {
    let head = format!(
        "{} → {}",
        update::short_commit(&pending.from),
        update::short_commit(&pending.to)
    );
    match pending.commits {
        Some(count) if pending.commits_truncated => format!("{head}（{count}+ 个提交）"),
        Some(count) => format!("{head}（{count} 个提交）"),
        None => head,
    }
}

pub(crate) fn uninstall_plugin(plugin_id: &str, config_dir: Option<&Path>) -> Result<(), String> {
    let config_dir = resolve_config_dir(config_dir)?;
    installer::uninstall(plugin_id, &config_dir).map_err(plugin_market_error_message)
}

pub(crate) fn grant_plugin_trust(
    plugin_id: &str,
    keys: Option<Vec<String>>,
    config_dir: Option<&Path>,
) -> Result<(), String> {
    let config_dir = resolve_config_dir(config_dir)?;
    let dir = plugins_root(&config_dir).join(plugin_id);
    let manifest = PluginManifest::load(&dir).map_err(plugin_market_error_message)?;
    let mut trust = TrustStore::load(&config_dir).map_err(plugin_market_error_message)?;
    match keys {
        Some(keys) => trust.grant_items(plugin_id, &keys, &manifest, &dir),
        None => trust.grant_all(plugin_id, &manifest, &dir),
    }
    trust.save().map_err(plugin_market_error_message)
}

pub(crate) fn list_installed_plugins(
    config_dir: Option<&Path>,
) -> Result<Vec<InstalledPluginSummary>, String> {
    let config_dir = resolve_config_dir(config_dir)?;
    let root = plugins_root(&config_dir);
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("无法读取插件目录：{error}")),
    };
    let trust = TrustStore::load(&config_dir).map_err(plugin_market_error_message)?;
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Ok(manifest) = PluginManifest::load(&dir) else {
            continue;
        };
        let Some(id) = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        out.push(summarize(id, &dir, manifest, &trust));
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(out)
}

pub(crate) fn plugin_trust_review(
    plugin_id: &str,
    config_dir: Option<&Path>,
) -> Result<PluginTrustReview, String> {
    let config_dir = resolve_config_dir(config_dir)?;
    let dir = plugins_root(&config_dir).join(plugin_id);
    let manifest = PluginManifest::load(&dir).map_err(plugin_market_error_message)?;
    let trust = TrustStore::load(&config_dir).map_err(plugin_market_error_message)?;
    let items = match trust.status(plugin_id, &manifest, &dir) {
        TrustStatus::Trusted => Vec::new(),
        TrustStatus::NeedsReview(items) | TrustStatus::Drifted(items) => items
            .into_iter()
            .map(|item| PluginTrustItem {
                key: item.key,
                content: item.content,
            })
            .collect(),
    };
    Ok(PluginTrustReview {
        plugin_id: plugin_id.to_string(),
        items,
    })
}

fn summarize(
    id: String,
    dir: &Path,
    manifest: PluginManifest,
    trust: &TrustStore,
) -> InstalledPluginSummary {
    let trust_state = match trust.status(&id, &manifest, dir) {
        TrustStatus::Trusted => PluginTrustState::Trusted,
        TrustStatus::NeedsReview(items) => {
            PluginTrustState::NeedsReview(items.into_iter().map(|item| item.key).collect())
        }
        TrustStatus::Drifted(items) => {
            PluginTrustState::Drifted(items.into_iter().map(|item| item.key).collect())
        }
    };
    let capabilities = manifest
        .capabilities
        .iter()
        .map(capability_summary)
        .collect();
    InstalledPluginSummary {
        id,
        repo: manifest.repo,
        reference: manifest.reference,
        installed_at_ms: manifest.installed_at,
        capabilities,
        trust: trust_state,
    }
}

fn capability_summary(capability: &Capability) -> PluginCapabilitySummary {
    let key = capability.trust_key();
    match capability {
        Capability::Skill { name, .. } => PluginCapabilitySummary {
            key,
            kind: PluginCapabilityKind::Skill,
            label: name.clone(),
            toggle_source_id: Some(format!("skill:{name}")),
        },
        Capability::Mcp { name, .. } => PluginCapabilitySummary {
            key,
            kind: PluginCapabilityKind::Mcp,
            label: name.clone(),
            toggle_source_id: Some(format!("mcp:{name}")),
        },
        Capability::Hook { event, tool, .. } => PluginCapabilitySummary {
            key,
            kind: PluginCapabilityKind::Hook,
            label: match tool {
                Some(tool) => format!("{event} · {tool}"),
                None => event.clone(),
            },
            // Hooks from installed plugins are not yet wired into the
            // engine's hook loader (`crate::hooks_config::load_hook_entries`
            // only reads the global/project `hooks.json`, not
            // `<config_dir>/plugins/*`), so there is no `SetIntegrationEnabled`
            // id for one yet - surfaced read-only until that core wiring
            // lands.
            toggle_source_id: None,
        },
    }
}

fn resolve_config_dir(config_dir: Option<&Path>) -> Result<PathBuf, String> {
    config_dir
        .map(Path::to_path_buf)
        .map(Ok)
        .unwrap_or_else(ConfigManager::config_dir)
        .map_err(|error| error.to_string())
}

/// Maps installer/trust failures to user-facing Chinese messages, per
/// `PluginMarketError`'s variants.
fn plugin_market_error_message(error: PluginMarketError) -> String {
    match error {
        PluginMarketError::NotFound(_) => "未找到该插件仓库，请检查来源或访问权限".to_string(),
        PluginMarketError::AuthRequired(_) => {
            "该仓库无法访问，请检查权限或改用可公开访问的仓库后重试".to_string()
        }
        PluginMarketError::Timeout => "Git 操作未能完成，请稍后重试".to_string(),
        PluginMarketError::GitMissing => "未检测到 git，请先安装 git 后重试".to_string(),
        PluginMarketError::AlreadyInstalled(id) => format!("插件已安装：{id}"),
        PluginMarketError::InvalidSpec(message) => format!("插件来源无效：{message}"),
        PluginMarketError::UnsafePath(message) => format!("插件来源不安全：{message}"),
        PluginMarketError::GitFailed(message) => format!("Git 操作失败：{message}"),
        PluginMarketError::NotInstalled(id) => format!("插件未安装：{id}"),
        PluginMarketError::NotGitBacked(id) => {
            format!("插件 {id} 不是通过 git 安装的，无法检查更新")
        }
        PluginMarketError::Io(error) => format!("文件操作失败：{error}"),
        PluginMarketError::Manifest(message) => format!("插件清单格式错误：{message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_fixture_repo(dir: &Path) {
        std::fs::create_dir_all(dir.join("skills/demo")).unwrap();
        std::fs::write(
            dir.join("skills/demo/SKILL.md"),
            "---\ndescription: demo skill\n---\nDo the demo.",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("hooks")).unwrap();
        std::fs::write(
            dir.join("hooks/hooks.json"),
            r#"{"hooks":[{"event":"before_tool_use","tool":"Bash","script":"check.sh"}]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("hooks/check.sh"), "#!/bin/sh\necho hi\n").unwrap();
        std::fs::write(
            dir.join(".mcp.json"),
            r#"{"mcpServers":{"srv":{"command":"npx","args":["-y","tool"]}}}"#,
        )
        .unwrap();
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .current_dir(dir)
                .args(args)
                .status()
                .expect("git available for fixture setup");
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        run(&["add", "-A"]);
        run(&["commit", "-q", "-m", "init"]);
    }

    #[tokio::test]
    async fn install_then_list_reports_all_capability_kinds_needing_review() {
        let fixture = tempfile::tempdir().unwrap();
        init_fixture_repo(fixture.path());
        let config_dir = tempfile::tempdir().unwrap();
        let spec = fixture.path().to_string_lossy().to_string();

        install_plugin(&spec, None, Some(config_dir.path()))
            .await
            .unwrap();

        let plugins = list_installed_plugins(Some(config_dir.path())).unwrap();
        assert_eq!(plugins.len(), 1);
        let plugin = &plugins[0];
        assert_eq!(plugin.capabilities.len(), 3);
        assert!(plugin
            .capabilities
            .iter()
            .any(|cap| cap.kind == PluginCapabilityKind::Skill
                && cap.toggle_source_id.as_deref() == Some("skill:demo")));
        assert!(plugin
            .capabilities
            .iter()
            .any(|cap| cap.kind == PluginCapabilityKind::Mcp
                && cap.toggle_source_id.as_deref() == Some("mcp:srv")));
        assert!(plugin
            .capabilities
            .iter()
            .any(|cap| cap.kind == PluginCapabilityKind::Hook && cap.toggle_source_id.is_none()));
        match &plugin.trust {
            PluginTrustState::NeedsReview(keys) => assert_eq!(keys.len(), 2),
            other => panic!("expected NeedsReview for the fresh install, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn trust_review_shows_verbatim_content_and_grant_all_clears_it() {
        let fixture = tempfile::tempdir().unwrap();
        init_fixture_repo(fixture.path());
        let config_dir = tempfile::tempdir().unwrap();
        let spec = fixture.path().to_string_lossy().to_string();
        install_plugin(&spec, None, Some(config_dir.path()))
            .await
            .unwrap();
        let plugin_id = &list_installed_plugins(Some(config_dir.path())).unwrap()[0]
            .id
            .clone();

        let review = plugin_trust_review(plugin_id, Some(config_dir.path())).unwrap();
        assert_eq!(review.items.len(), 2);
        assert!(review
            .items
            .iter()
            .any(|item| item.content == "npx -y tool"));
        assert!(review
            .items
            .iter()
            .any(|item| item.content.contains("echo hi")));

        grant_plugin_trust(plugin_id, None, Some(config_dir.path())).unwrap();
        let plugins = list_installed_plugins(Some(config_dir.path())).unwrap();
        assert_eq!(plugins[0].trust, PluginTrustState::Trusted);
    }

    #[tokio::test]
    async fn grant_items_trusts_only_the_named_capability() {
        let fixture = tempfile::tempdir().unwrap();
        init_fixture_repo(fixture.path());
        let config_dir = tempfile::tempdir().unwrap();
        let spec = fixture.path().to_string_lossy().to_string();
        install_plugin(&spec, None, Some(config_dir.path()))
            .await
            .unwrap();
        let plugin_id = list_installed_plugins(Some(config_dir.path())).unwrap()[0]
            .id
            .clone();

        grant_plugin_trust(
            &plugin_id,
            Some(vec!["mcp:srv".to_string()]),
            Some(config_dir.path()),
        )
        .unwrap();

        match &list_installed_plugins(Some(config_dir.path())).unwrap()[0].trust {
            PluginTrustState::NeedsReview(keys) => {
                assert_eq!(
                    keys,
                    &vec!["hook:before_tool_use:Bash:hooks/check.sh".to_string()]
                );
            }
            other => panic!("expected the ungranted hook to still need review, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn uninstall_removes_the_plugin_and_its_trust_record() {
        let fixture = tempfile::tempdir().unwrap();
        init_fixture_repo(fixture.path());
        let config_dir = tempfile::tempdir().unwrap();
        let spec = fixture.path().to_string_lossy().to_string();
        install_plugin(&spec, None, Some(config_dir.path()))
            .await
            .unwrap();
        let plugin_id = list_installed_plugins(Some(config_dir.path())).unwrap()[0]
            .id
            .clone();
        grant_plugin_trust(&plugin_id, None, Some(config_dir.path())).unwrap();

        uninstall_plugin(&plugin_id, Some(config_dir.path())).unwrap();
        assert!(list_installed_plugins(Some(config_dir.path()))
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn update_check_then_apply_resyncs_and_re_gates_the_changed_hook() {
        let fixture = tempfile::tempdir().unwrap();
        init_fixture_repo(fixture.path());
        let config_dir = tempfile::tempdir().unwrap();
        let spec = fixture.path().to_string_lossy().to_string();
        install_plugin(&spec, None, Some(config_dir.path()))
            .await
            .unwrap();
        let plugin_id = list_installed_plugins(Some(config_dir.path())).unwrap()[0]
            .id
            .clone();
        grant_plugin_trust(&plugin_id, None, Some(config_dir.path())).unwrap();

        assert_eq!(
            check_plugin_update(&plugin_id, Some(config_dir.path()))
                .await
                .unwrap()
                .available,
            None
        );

        std::fs::write(
            fixture.path().join("hooks/check.sh"),
            "#!/bin/sh\necho rewritten\n",
        )
        .unwrap();
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .current_dir(fixture.path())
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        };
        run(&["add", "-A"]);
        run(&["commit", "-q", "-m", "rewrite the hook"]);

        let check = check_plugin_update(&plugin_id, Some(config_dir.path()))
            .await
            .unwrap();
        let available = check.available.expect("an update should be available");
        assert!(available.summary.contains('→'));
        assert!(available.summary.contains("1 个提交"), "{available:?}");

        apply_plugin_update(&plugin_id, Some(config_dir.path()))
            .await
            .unwrap();

        match &list_installed_plugins(Some(config_dir.path())).unwrap()[0].trust {
            PluginTrustState::Drifted(keys) => {
                assert_eq!(
                    keys,
                    &vec!["hook:before_tool_use:Bash:hooks/check.sh".to_string()]
                );
            }
            other => panic!("expected the rewritten hook to need re-review, got {other:?}"),
        }
        let review = plugin_trust_review(&plugin_id, Some(config_dir.path())).unwrap();
        assert!(review
            .items
            .iter()
            .any(|item| item.content.contains("rewritten")));
    }

    #[tokio::test]
    async fn update_check_on_a_hand_placed_plugin_dir_is_reported_in_chinese() {
        let config_dir = tempfile::tempdir().unwrap();
        let dir = plugins_root(config_dir.path()).join("acme__manual");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"repo":"acme/manual","ref":"HEAD","installedAt":0,"capabilities":[]}"#,
        )
        .unwrap();

        let error = check_plugin_update("acme__manual", Some(config_dir.path()))
            .await
            .unwrap_err();
        assert!(error.contains("无法检查更新"), "{error}");
    }

    #[tokio::test]
    async fn install_failure_is_reported_in_chinese() {
        let config_dir = tempfile::tempdir().unwrap();
        let error = install_plugin("../../evil", None, Some(config_dir.path()))
            .await
            .unwrap_err();
        assert!(error.contains("插件来源无效"));
    }

    #[tokio::test]
    async fn double_install_reports_already_installed_in_chinese() {
        let fixture = tempfile::tempdir().unwrap();
        init_fixture_repo(fixture.path());
        let config_dir = tempfile::tempdir().unwrap();
        let spec = fixture.path().to_string_lossy().to_string();
        install_plugin(&spec, None, Some(config_dir.path()))
            .await
            .unwrap();
        let error = install_plugin(&spec, None, Some(config_dir.path()))
            .await
            .unwrap_err();
        assert!(error.contains("插件已安装"));
    }
}
