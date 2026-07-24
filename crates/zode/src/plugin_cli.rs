use std::path::{Path, PathBuf};

use zode_core::plugin_package::PluginPackageManager;

use crate::args::{MarketplaceCommand, PluginCommand};

pub fn run(action: &PluginCommand, cwd: &Path) -> i32 {
    match execute(action, cwd) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            0
        }
        Err(error) => {
            eprintln!("zode plugin: {error}");
            1
        }
    }
}

fn execute(action: &PluginCommand, cwd: &Path) -> Result<String, zode_core::CoreError> {
    let manager = PluginPackageManager::open_default()?;
    match action {
        PluginCommand::List { json, available } => {
            let installed = manager
                .registry()?
                .plugins
                .into_values()
                .collect::<Vec<_>>();
            if *json {
                let available = if *available {
                    Some(manager.marketplace_plugins()?)
                } else {
                    None
                };
                return Ok(serde_json::to_string_pretty(&serde_json::json!({
                    "installed": installed,
                    "available": available,
                }))?);
            }
            if *available {
                return Err(zode_core::CoreError::Other(
                    "--available requires --json".into(),
                ));
            }
            Ok(installed
                .into_iter()
                .map(|plugin| {
                    format!(
                        "{}\t{}\t{}\t{}\t{}",
                        plugin.name,
                        plugin.version.as_deref().unwrap_or("-"),
                        if plugin.enabled {
                            "enabled"
                        } else {
                            "disabled"
                        },
                        plugin.sha256,
                        plugin.source
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        PluginCommand::Validate { path } => {
            let path = resolve_path(cwd, path);
            Ok(serde_json::to_string_pretty(&manager.validate(&path)?)?)
        }
        PluginCommand::Install { source, trust } => {
            let installed = manager.install(source, cwd, *trust)?;
            warn_permissions(std::slice::from_ref(&installed));
            Ok(serde_json::to_string_pretty(&installed)?)
        }
        PluginCommand::Update { name, trust } => {
            let updated = manager.update(name.as_deref(), cwd, *trust)?;
            warn_permissions(&updated);
            Ok(serde_json::to_string_pretty(&updated)?)
        }
        PluginCommand::Uninstall { name, keep_data } => Ok(serde_json::to_string_pretty(
            &manager.uninstall(name, *keep_data)?,
        )?),
        PluginCommand::Enable { name } => Ok(serde_json::to_string_pretty(
            &manager.set_enabled(name, true)?,
        )?),
        PluginCommand::Disable { name } => Ok(serde_json::to_string_pretty(
            &manager.set_enabled(name, false)?,
        )?),
        PluginCommand::Details { name } => {
            let plugin = manager.registry()?.plugins.remove(name).ok_or_else(|| {
                zode_core::CoreError::Other(format!("installed plugin not found: {name}"))
            })?;
            Ok(serde_json::to_string_pretty(&plugin)?)
        }
        PluginCommand::Marketplace { action } => marketplace(&manager, action, cwd),
    }
}

fn marketplace(
    manager: &PluginPackageManager,
    action: &MarketplaceCommand,
    cwd: &Path,
) -> Result<String, zode_core::CoreError> {
    match action {
        MarketplaceCommand::List { json } => {
            let sources = manager.marketplace_registry()?.sources;
            let plugins = manager.marketplace_plugins()?;
            if *json {
                Ok(serde_json::to_string_pretty(&serde_json::json!({
                    "sources": sources,
                    "plugins": plugins,
                }))?)
            } else {
                let mut lines = sources
                    .values()
                    .map(|source| {
                        format!(
                            "source\t{}\t{}\t{}",
                            source.name, source.updated_at, source.source
                        )
                    })
                    .collect::<Vec<_>>();
                lines.extend(plugins.into_iter().map(|plugin| {
                    format!(
                        "plugin\t{}@{}\t{}\t{}",
                        plugin.name,
                        plugin.marketplace,
                        plugin.version.as_deref().unwrap_or("-"),
                        plugin.description.as_deref().unwrap_or("")
                    )
                }));
                Ok(lines.join("\n"))
            }
        }
        MarketplaceCommand::Add { source, trust } => Ok(serde_json::to_string_pretty(
            &manager.marketplace_add(source, cwd, *trust)?,
        )?),
        MarketplaceCommand::Update { name } => Ok(serde_json::to_string_pretty(
            &manager.marketplace_update(name.as_deref(), cwd)?,
        )?),
        MarketplaceCommand::Remove { name } => Ok(serde_json::to_string_pretty(
            &manager.marketplace_remove(name)?,
        )?),
    }
}

/// Surface each plugin's declared capability grant on stderr, so a `--trust`
/// install/update is informed consent rather than a blind flag. Stdout keeps
/// the stable machine-readable JSON (which now also carries `permissions`).
fn warn_permissions(plugins: &[zode_core::plugin_package::InstalledPlugin]) {
    for plugin in plugins {
        if !plugin.permissions.is_empty() {
            eprintln!(
                "zode plugin: {} may use {}",
                plugin.name,
                plugin.permissions.summary()
            );
        }
    }
}

fn resolve_path(cwd: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}
