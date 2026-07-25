//! Installable plugin packages and static marketplace sources.
//!
//! This module intentionally has no hosted marketplace dependency. Sources are
//! local directories or Git repositories; a marketplace is just a cached,
//! inspectable directory/index. Installs are copied snapshots with provenance
//! and a content hash so runtime discovery never executes from a mutable clone.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::ConfigManager;
use crate::sessions::journal::write_json_atomic;
use crate::CoreError;

mod support;

pub use support::validate_plugin;
use support::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginAuthor {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PluginPaths {
    One(String),
    Many(Vec<String>),
}

impl PluginPaths {
    fn values(&self) -> Vec<&str> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PluginPathOrInline {
    Path(String),
    Paths(Vec<String>),
    Inline(serde_json::Value),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginUiManifest {
    /// JavaScript file that registers a renderer with `zode.ui.sidebar(fn)`.
    #[serde(default)]
    pub sidebar: Option<String>,
    /// JavaScript file that registers a renderer with
    /// `zode.ui.statusLine(fn)`.
    #[serde(default)]
    pub status_line: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPermissions {
    /// HTTPS hostnames the plugin may contact. A leading `*.` matches
    /// subdomains but never the bare suffix.
    #[serde(default)]
    pub network: Vec<String>,
    /// Environment variables that may be used as opaque request-header
    /// secrets. Their values are never exposed to JavaScript.
    #[serde(default)]
    pub env: Vec<String>,
    /// Optional richer UI-context sections. Base session/model/usage/status
    /// fields are always present.
    #[serde(default)]
    pub context: Vec<String>,
}

impl PluginPermissions {
    pub fn is_empty(&self) -> bool {
        self.network.is_empty() && self.env.is_empty() && self.context.is_empty()
    }

    /// True when this permission set requests nothing beyond `granted`.
    /// Comparison is by exact entry, deliberately conservative: narrowing a
    /// wildcard still counts as a change and requires fresh consent.
    pub fn is_subset_of(&self, granted: &Self) -> bool {
        self.network
            .iter()
            .all(|host| granted.network.contains(host))
            && self.env.iter().all(|name| granted.env.contains(name))
            && self
                .context
                .iter()
                .all(|scope| granted.context.contains(scope))
    }

    /// One-line human-readable summary for consent surfaces.
    pub fn summary(&self) -> String {
        format!(
            "network={:?} env={:?} context={:?}",
            self.network, self.env, self.context
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub name: String,
    /// Claude Code compatibility: install disabled unless the user has already
    /// made an explicit enable/disable choice for this plugin.
    #[serde(default)]
    pub default_enabled: Option<bool>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<PluginAuthor>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub skills: Option<PluginPaths>,
    #[serde(default)]
    pub commands: Option<PluginPaths>,
    #[serde(default)]
    pub agents: Option<PluginPaths>,
    #[serde(default)]
    pub hooks: Option<PluginPathOrInline>,
    #[serde(default)]
    pub mcp_servers: Option<PluginPathOrInline>,
    #[serde(default)]
    pub lsp_servers: Option<PluginPathOrInline>,
    #[serde(default)]
    pub ui: Option<PluginUiManifest>,
    #[serde(default)]
    pub permissions: PluginPermissions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedPlugin {
    pub root: PathBuf,
    pub manifest_path: Option<PathBuf>,
    pub manifest: PluginManifest,
    pub components: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPlugin {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub source: String,
    pub source_ref: Option<String>,
    pub source_subdir: Option<String>,
    pub path: PathBuf,
    pub sha256: String,
    pub trusted: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub installed_at: u64,
    pub updated_at: u64,
    pub components: Vec<String>,
    /// Permissions declared by the installed manifest snapshot, recorded so
    /// list/details can surface them and updates can detect expansion.
    #[serde(default)]
    pub permissions: PluginPermissions,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallRegistry {
    #[serde(default = "registry_schema")]
    pub schema_version: u32,
    #[serde(default)]
    pub plugins: BTreeMap<String, InstalledPlugin>,
}

fn registry_schema() -> u32 {
    1
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSource {
    pub name: String,
    pub source: String,
    pub cache_path: PathBuf,
    pub added_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceRegistry {
    #[serde(default = "registry_schema")]
    pub schema_version: u32,
    #[serde(default)]
    pub sources: BTreeMap<String, MarketplaceSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplacePlugin {
    pub marketplace: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub path: PathBuf,
    pub components: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum PackageDirectoryKind {
    Skills,
    Commands,
    Agents,
}

#[derive(Debug, Clone, Copy)]
pub enum PackageConfigKind {
    Hooks,
    Mcp,
    Lsp,
}

#[derive(Debug, Clone)]
pub struct PackageConfigComponent {
    pub plugin: String,
    pub root: PathBuf,
    pub path: Option<PathBuf>,
    pub inline: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct PackageUiRenderer {
    pub plugin: String,
    pub path: PathBuf,
    pub slot: PackageUiSlot,
    pub permissions: PluginPermissions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageUiSlot {
    Sidebar,
    StatusLine,
}

impl PackageConfigComponent {
    /// Load the component's JSON: an inline value wins, else the referenced
    /// file is read and parsed. Failures warn (with the plugin name) and
    /// return None — a plugin's broken config must never break assembly, but
    /// it must not disappear silently either.
    pub fn load_json(&self) -> Option<serde_json::Value> {
        let mut value = if let Some(value) = &self.inline {
            value.clone()
        } else {
            let path = self.path.as_ref()?;
            let text = match std::fs::read_to_string(path) {
                Ok(text) => text,
                Err(error) => {
                    tracing::warn!(plugin = %self.plugin, path = %path.display(),
                        "skip unreadable plugin config: {error}");
                    return None;
                }
            };
            match serde_json::from_str(&text) {
                Ok(value) => value,
                Err(error) => {
                    tracing::warn!(plugin = %self.plugin, path = %path.display(),
                        "skip malformed plugin config: {error}");
                    return None;
                }
            }
        };
        expand_plugin_variables(&mut value, &self.plugin, &self.root);
        Some(value)
    }
}

fn expand_plugin_variables(value: &mut serde_json::Value, plugin: &str, root: &Path) {
    match value {
        serde_json::Value::String(text) => {
            let root_text = root.display().to_string();
            let data = root
                .parent()
                .and_then(Path::parent)
                .map(|config_root| config_root.join("plugin-data").join(plugin))
                .unwrap_or_else(|| root.join(".data"));
            let data_text = data.display().to_string();
            for variable in [
                "ZODE_PLUGIN_ROOT",
                "CLAUDE_PLUGIN_ROOT",
                "CODEX_PLUGIN_ROOT",
                "GROK_PLUGIN_ROOT",
            ] {
                *text = text.replace(&format!("${{{variable}}}"), &root_text);
            }
            for variable in [
                "ZODE_PLUGIN_DATA",
                "CLAUDE_PLUGIN_DATA",
                "CODEX_PLUGIN_DATA",
                "GROK_PLUGIN_DATA",
            ] {
                *text = text.replace(&format!("${{{variable}}}"), &data_text);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                expand_plugin_variables(value, plugin, root);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                expand_plugin_variables(value, plugin, root);
            }
        }
        _ => {}
    }
}

/// Resolve enabled managed package directories declared by manifests. Runtime
/// discovery calls this in addition to convention-based tree scans.
pub fn installed_package_dirs(kind: PackageDirectoryKind) -> Vec<PathBuf> {
    enabled_validated_plugins()
        .into_iter()
        .flat_map(|plugin| {
            let (configured, default) = match kind {
                PackageDirectoryKind::Skills => (plugin.manifest.skills.as_ref(), "skills"),
                PackageDirectoryKind::Commands => (plugin.manifest.commands.as_ref(), "commands"),
                PackageDirectoryKind::Agents => (plugin.manifest.agents.as_ref(), "agents"),
            };
            configured
                .map(PluginPaths::values)
                .unwrap_or_else(|| vec![default])
                .into_iter()
                .map(|relative| plugin.root.join(relative))
                .filter(|path| path.is_dir())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Resolve enabled managed package JSON components, including inline manifest
/// values. Paths were containment-checked at install time and are revalidated
/// each time the package is loaded.
pub fn installed_package_configs(kind: PackageConfigKind) -> Vec<PackageConfigComponent> {
    enabled_validated_plugins()
        .into_iter()
        .flat_map(|plugin| {
            let (configured, default) = match kind {
                PackageConfigKind::Hooks => (plugin.manifest.hooks.as_ref(), "hooks/hooks.json"),
                PackageConfigKind::Mcp => (plugin.manifest.mcp_servers.as_ref(), ".mcp.json"),
                PackageConfigKind::Lsp => (plugin.manifest.lsp_servers.as_ref(), ".lsp.json"),
            };
            let sources = match configured {
                Some(PluginPathOrInline::Path(relative)) => {
                    vec![(Some(plugin.root.join(relative)), None)]
                }
                Some(PluginPathOrInline::Paths(relative)) => relative
                    .iter()
                    .map(|relative| (Some(plugin.root.join(relative)), None))
                    .collect(),
                Some(PluginPathOrInline::Inline(value)) => vec![(None, Some(value.clone()))],
                None => {
                    let path = plugin.root.join(default);
                    vec![(path.is_file().then_some(path), None)]
                }
            };
            sources
                .into_iter()
                .filter_map(move |(path, inline)| {
                    (path.is_some() || inline.is_some()).then_some(PackageConfigComponent {
                        plugin: plugin.manifest.name.clone(),
                        root: plugin.root.clone(),
                        path,
                        inline,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Every installed package (enabled or not), flattened for the `/plugin`
/// picker. Best-effort: an unreadable registry yields an empty list rather
/// than failing the caller, exactly like the discovery helpers above.
pub fn installed_package_entries() -> Vec<crate::plugin::PackageEntry> {
    let Ok(manager) = PluginPackageManager::open_default() else {
        return Vec::new();
    };
    let Ok(registry) = manager.registry() else {
        return Vec::new();
    };
    registry
        .plugins
        .into_values()
        .map(|record| {
            let description = record
                .description
                .filter(|d| !d.trim().is_empty())
                .unwrap_or_else(|| {
                    if record.components.is_empty() {
                        "plugin package".to_string()
                    } else {
                        record.components.join(", ")
                    }
                });
            crate::plugin::PackageEntry {
                // An untrusted package contributes nothing at runtime even when
                // the registry says enabled — say so instead of silently
                // rendering it as a working plugin.
                description: if record.trusted {
                    description
                } else {
                    format!("(untrusted) {description}")
                },
                detail: record.version.unwrap_or_else(|| "plugin".to_string()),
                enabled: record.enabled,
                name: record.name,
            }
        })
        .collect()
}

/// Resolve trusted, enabled JavaScript renderers declared by managed plugins.
/// Paths are containment-checked during validation and revalidated whenever
/// the registry is loaded.
pub fn installed_ui_renderers() -> Vec<PackageUiRenderer> {
    enabled_validated_plugins()
        .into_iter()
        .flat_map(|plugin| {
            let Some(ui) = plugin.manifest.ui.as_ref() else {
                return Vec::new();
            };
            [
                (ui.sidebar.as_ref(), PackageUiSlot::Sidebar),
                (ui.status_line.as_ref(), PackageUiSlot::StatusLine),
            ]
            .into_iter()
            .filter_map(|(relative, slot)| {
                let path = plugin.root.join(relative?);
                path.is_file().then_some(PackageUiRenderer {
                    plugin: plugin.manifest.name.clone(),
                    path,
                    slot,
                    permissions: plugin.manifest.permissions.clone(),
                })
            })
            .collect::<Vec<_>>()
        })
        .collect()
}

fn enabled_validated_plugins() -> Vec<ValidatedPlugin> {
    let Ok(manager) = PluginPackageManager::open_default() else {
        return Vec::new();
    };
    let Ok(registry) = manager.registry() else {
        return Vec::new();
    };
    registry
        .plugins
        .values()
        .filter(|record| record.enabled && record.trusted)
        .filter_map(|record| match validate_plugin(&record.path) {
            Ok(plugin) => Some(plugin),
            Err(error) => {
                tracing::warn!(plugin = %record.name, "skip invalid installed plugin: {error}");
                None
            }
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct PluginPackageManager {
    config_root: PathBuf,
    plugins_root: PathBuf,
    disabled_plugins_root: PathBuf,
    marketplaces_root: PathBuf,
}

impl PluginPackageManager {
    pub fn open_default() -> Result<Self, CoreError> {
        Self::at(ConfigManager::config_dir()?)
    }

    pub fn at(config_root: PathBuf) -> Result<Self, CoreError> {
        let plugins_root = config_root.join("plugins");
        let disabled_plugins_root = config_root.join("plugins-disabled");
        let marketplaces_root = config_root.join("marketplaces");
        std::fs::create_dir_all(&plugins_root)?;
        std::fs::create_dir_all(&disabled_plugins_root)?;
        std::fs::create_dir_all(&marketplaces_root)?;
        Ok(Self {
            config_root,
            plugins_root,
            disabled_plugins_root,
            marketplaces_root,
        })
    }

    pub fn validate(&self, path: &Path) -> Result<ValidatedPlugin, CoreError> {
        validate_plugin(path)
    }

    pub fn registry(&self) -> Result<PluginInstallRegistry, CoreError> {
        read_json_or_default(&self.registry_path())
    }

    pub fn install(
        &self,
        source: &str,
        cwd: &Path,
        trust: bool,
    ) -> Result<InstalledPlugin, CoreError> {
        if !trust {
            return Err(CoreError::Other(
                "plugin install requires --trust because plugins may activate hooks, MCP servers, commands, skills, UI scripts, or network access"
                    .into(),
            ));
        }
        if let Some(entry) = self.resolve_marketplace_plugin(source)? {
            // A fresh --trust install consents to whatever the manifest
            // declares, so permission widening does not apply here.
            return self.install_source(&entry.path.display().to_string(), cwd, trust, false, true);
        }
        self.install_source(source, cwd, trust, false, true)
    }

    /// Update installed plugin(s) from their recorded source. The source can
    /// be a moving ref (e.g. a Git branch), so an update that requests
    /// *broader* manifest permissions than the installed snapshot is refused
    /// unless `trust` re-consents — otherwise a routine update could silently
    /// gain new network hosts or secret env vars.
    pub fn update(
        &self,
        name: Option<&str>,
        cwd: &Path,
        trust: bool,
    ) -> Result<Vec<InstalledPlugin>, CoreError> {
        let registry = self.registry()?;
        let targets: Vec<InstalledPlugin> =
            match name {
                Some(name) => vec![registry.plugins.get(name).cloned().ok_or_else(|| {
                    CoreError::Other(format!("installed plugin not found: {name}"))
                })?],
                None => registry.plugins.values().cloned().collect(),
            };
        let mut updated = Vec::new();
        for target in targets {
            updated.push(self.install_source(&target.source, cwd, true, true, trust)?);
        }
        Ok(updated)
    }

    pub fn uninstall(&self, name: &str, keep_data: bool) -> Result<InstalledPlugin, CoreError> {
        validate_name(name)?;
        let mut registry = self.registry()?;
        let installed = registry
            .plugins
            .remove(name)
            .ok_or_else(|| CoreError::Other(format!("installed plugin not found: {name}")))?;
        let managed_root = if installed.enabled {
            self.plugins_root.canonicalize()?
        } else {
            self.disabled_plugins_root.canonicalize()?
        };
        let path = installed.path.canonicalize()?;
        if !path.starts_with(&managed_root) || path == managed_root {
            return Err(CoreError::Other(format!(
                "refusing to remove unmanaged plugin path: {}",
                path.display()
            )));
        }
        std::fs::remove_dir_all(&path)?;
        self.save_registry(&registry)?;
        if !keep_data {
            let data = self.config_root.join("plugin-data").join(name);
            if data.is_dir() {
                std::fs::remove_dir_all(data)?;
            }
        }
        Ok(installed)
    }

    pub fn set_enabled(&self, name: &str, enabled: bool) -> Result<InstalledPlugin, CoreError> {
        validate_name(name)?;
        let mut registry = self.registry()?;
        let record = registry
            .plugins
            .get_mut(name)
            .ok_or_else(|| CoreError::Other(format!("installed plugin not found: {name}")))?;
        if record.enabled == enabled {
            return Ok(record.clone());
        }
        let destination = if enabled {
            self.plugins_root.join(name)
        } else {
            self.disabled_plugins_root.join(name)
        };
        if destination.exists() {
            return Err(CoreError::Other(format!(
                "plugin destination already exists: {}",
                destination.display()
            )));
        }
        std::fs::rename(&record.path, &destination)?;
        record.path = destination;
        record.enabled = enabled;
        record.updated_at = now_secs();
        let result = record.clone();
        self.save_registry(&registry)?;
        Ok(result)
    }

    pub fn marketplace_registry(&self) -> Result<MarketplaceRegistry, CoreError> {
        read_json_or_default(&self.marketplace_registry_path())
    }

    pub fn marketplace_add(
        &self,
        source: &str,
        cwd: &Path,
        trust: bool,
    ) -> Result<MarketplaceSource, CoreError> {
        if !trust {
            return Err(CoreError::Other(
                "marketplace add requires --trust; its entries can install executable plugin components"
                    .into(),
            ));
        }
        let spec = SourceSpec::parse(source, cwd)?;
        let name = marketplace_name(&spec.display_source);
        let mut registry = self.marketplace_registry()?;
        if registry.sources.contains_key(&name) {
            return Err(CoreError::Other(format!(
                "marketplace already exists: {name}"
            )));
        }
        let cache_path = self.marketplaces_root.join(&name);
        materialize_source(&spec, &cache_path)?;
        let now = now_secs();
        let entry = MarketplaceSource {
            name: name.clone(),
            source: spec.display_source,
            cache_path,
            added_at: now,
            updated_at: now,
        };
        registry.sources.insert(name, entry.clone());
        self.save_marketplace_registry(&registry)?;
        Ok(entry)
    }

    pub fn marketplace_remove(&self, name: &str) -> Result<MarketplaceSource, CoreError> {
        let mut registry = self.marketplace_registry()?;
        let entry = registry
            .sources
            .remove(name)
            .ok_or_else(|| CoreError::Other(format!("marketplace not found: {name}")))?;
        let managed_root = self.marketplaces_root.canonicalize()?;
        let path = entry.cache_path.canonicalize()?;
        if path.starts_with(&managed_root) && path != managed_root {
            std::fs::remove_dir_all(path)?;
        }
        self.save_marketplace_registry(&registry)?;
        Ok(entry)
    }

    pub fn marketplace_update(
        &self,
        name: Option<&str>,
        cwd: &Path,
    ) -> Result<Vec<MarketplaceSource>, CoreError> {
        let mut registry = self.marketplace_registry()?;
        let names: Vec<String> = match name {
            Some(name) if registry.sources.contains_key(name) => vec![name.to_string()],
            Some(name) => return Err(CoreError::Other(format!("marketplace not found: {name}"))),
            None => registry.sources.keys().cloned().collect(),
        };
        let mut updated = Vec::new();
        for name in names {
            let entry = registry.sources.get(&name).cloned().unwrap();
            let spec = SourceSpec::parse(&entry.source, cwd)?;
            materialize_source(&spec, &entry.cache_path)?;
            let mut refreshed = entry;
            refreshed.updated_at = now_secs();
            registry.sources.insert(name, refreshed.clone());
            updated.push(refreshed);
        }
        self.save_marketplace_registry(&registry)?;
        Ok(updated)
    }

    pub fn marketplace_plugins(&self) -> Result<Vec<MarketplacePlugin>, CoreError> {
        let registry = self.marketplace_registry()?;
        let mut entries = Vec::new();
        for source in registry.sources.values() {
            entries.extend(scan_marketplace(source)?);
        }
        entries.sort_by(|a, b| (&a.marketplace, &a.name).cmp(&(&b.marketplace, &b.name)));
        Ok(entries)
    }

    fn resolve_marketplace_plugin(
        &self,
        reference: &str,
    ) -> Result<Option<MarketplacePlugin>, CoreError> {
        if reference.contains('/')
            || reference.contains('\\')
            || reference.contains("://")
            || Path::new(reference).exists()
        {
            return Ok(None);
        }
        let (name, qualifier) = reference
            .split_once('@')
            .map_or((reference, None), |(name, source)| (name, Some(source)));
        let matches: Vec<_> = self
            .marketplace_plugins()?
            .into_iter()
            .filter(|entry| {
                entry.name == name && qualifier.is_none_or(|source| entry.marketplace == source)
            })
            .collect();
        match matches.len() {
            0 => Ok(None),
            1 => Ok(matches.into_iter().next()),
            _ => Err(CoreError::Other(format!(
                "marketplace plugin {name:?} is ambiguous; use {name}@<marketplace>"
            ))),
        }
    }

    fn install_source(
        &self,
        source: &str,
        cwd: &Path,
        trusted: bool,
        replace: bool,
        allow_broader_permissions: bool,
    ) -> Result<InstalledPlugin, CoreError> {
        let spec = SourceSpec::parse(source, cwd)?;
        let fetch = self
            .plugins_root
            .join(format!(".fetch-{}", Uuid::new_v4().simple()));
        materialize_source(&spec, &fetch)?;
        let plugin_root = locate_plugin_root(&fetch, spec.subdir.as_deref())?;
        let validated = validate_plugin(&plugin_root)?;
        let name = validated.manifest.name.clone();
        let mut registry = self.registry()?;
        if !allow_broader_permissions {
            if let Some(record) = registry.plugins.get(&name) {
                // Compare against the *installed snapshot's* manifest, not the
                // registry record: pre-permissions registries recorded nothing,
                // and the snapshot is the state the user actually trusted. An
                // unreadable snapshot fails closed to "nothing granted".
                let granted = validate_plugin(&record.path)
                    .map(|plugin| plugin.manifest.permissions)
                    .unwrap_or_default();
                if !validated.manifest.permissions.is_subset_of(&granted) {
                    cleanup_dir(&fetch);
                    return Err(CoreError::Other(format!(
                        "plugin {name} requests broader permissions than the installed \
                         version: {} (installed: {}); rerun with --trust to accept",
                        validated.manifest.permissions.summary(),
                        granted.summary(),
                    )));
                }
            }
        }
        let enabled = registry.plugins.get(&name).map_or(
            validated.manifest.default_enabled.unwrap_or(true),
            |record| record.enabled,
        );
        let destination = if enabled {
            self.plugins_root.join(&name)
        } else {
            self.disabled_plugins_root.join(&name)
        };
        if destination.exists() && !replace {
            cleanup_dir(&fetch);
            return Err(CoreError::Other(format!(
                "plugin already installed: {name}; use `zode plugin update {name}`"
            )));
        }
        let stage = self
            .plugins_root
            .join(format!(".stage-{name}-{}", Uuid::new_v4().simple()));
        copy_dir_snapshot(&plugin_root, &stage)?;
        let sha256 = hash_tree(&stage)?;
        let backup = self
            .plugins_root
            .join(format!(".backup-{name}-{}", Uuid::new_v4().simple()));
        if destination.exists() {
            std::fs::rename(&destination, &backup)?;
        }
        if let Err(error) = std::fs::rename(&stage, &destination) {
            if backup.exists() {
                let _ = std::fs::rename(&backup, &destination);
            }
            cleanup_dir(&fetch);
            cleanup_dir(&stage);
            return Err(error.into());
        }
        let now = now_secs();
        let installed_at = registry
            .plugins
            .get(&name)
            .map_or(now, |record| record.installed_at);
        let record = InstalledPlugin {
            name: name.clone(),
            version: validated.manifest.version,
            description: validated.manifest.description,
            source: spec.display_source,
            source_ref: spec.git_ref,
            source_subdir: spec.subdir,
            path: destination.clone(),
            sha256,
            trusted,
            enabled,
            installed_at,
            updated_at: now,
            components: validated.components,
            permissions: validated.manifest.permissions,
        };
        registry.plugins.insert(name, record.clone());
        if let Err(error) = self.save_registry(&registry) {
            cleanup_dir(&destination);
            if backup.exists() {
                let _ = std::fs::rename(&backup, &destination);
            }
            cleanup_dir(&fetch);
            return Err(error);
        }
        cleanup_dir(&backup);
        cleanup_dir(&fetch);
        Ok(record)
    }

    fn registry_path(&self) -> PathBuf {
        self.plugins_root.join("registry.json")
    }

    fn save_registry(&self, registry: &PluginInstallRegistry) -> Result<(), CoreError> {
        write_json_atomic(&self.registry_path(), registry)
    }

    fn marketplace_registry_path(&self) -> PathBuf {
        self.marketplaces_root.join("registry.json")
    }

    fn save_marketplace_registry(&self, registry: &MarketplaceRegistry) -> Result<(), CoreError> {
        write_json_atomic(&self.marketplace_registry_path(), registry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_load_json_prefers_inline_then_reads_path() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cfg.json");
        std::fs::write(&file, "{\"fromFile\":true}").unwrap();
        let component = |inline, path| PackageConfigComponent {
            plugin: "demo".into(),
            root: dir.path().to_path_buf(),
            path,
            inline,
        };
        assert_eq!(
            component(Some(serde_json::json!({"inline": 1})), Some(file.clone())).load_json(),
            Some(serde_json::json!({"inline": 1}))
        );
        assert_eq!(
            component(None, Some(file)).load_json(),
            Some(serde_json::json!({"fromFile": true}))
        );
        assert_eq!(
            component(None, Some(dir.path().join("missing.json"))).load_json(),
            None
        );
        assert_eq!(component(None, None).load_json(), None);
    }

    #[test]
    fn component_load_json_expands_cross_agent_plugin_variables() {
        let config = tempfile::tempdir().unwrap();
        let root = config.path().join("plugins/demo");
        std::fs::create_dir_all(&root).unwrap();
        let component = PackageConfigComponent {
            plugin: "demo".into(),
            root: root.clone(),
            path: None,
            inline: Some(serde_json::json!({
                "command": "${CLAUDE_PLUGIN_ROOT}/bin/server",
                "args": ["${CODEX_PLUGIN_ROOT}/config.json"],
                "data": "${CLAUDE_PLUGIN_DATA}/cache"
            })),
        };
        let value = component.load_json().unwrap();
        assert_eq!(value["command"], format!("{}/bin/server", root.display()));
        assert_eq!(value["args"][0], format!("{}/config.json", root.display()));
        assert_eq!(
            value["data"],
            format!("{}/plugin-data/demo/cache", config.path().display())
        );
    }

    fn write_plugin(root: &Path, version: &str) {
        std::fs::create_dir_all(root.join("skills/demo")).unwrap();
        std::fs::write(root.join("skills/demo/SKILL.md"), "# Demo").unwrap();
        std::fs::write(
            root.join("plugin.json"),
            serde_json::json!({"name":"demo-plugin", "version":version}).to_string(),
        )
        .unwrap();
    }

    #[test]
    fn validates_convention_and_manifest_components() {
        let temp = tempfile::tempdir().unwrap();
        write_plugin(temp.path(), "1.0.0");
        let plugin = validate_plugin(temp.path()).unwrap();
        assert_eq!(plugin.manifest.name, "demo-plugin");
        assert_eq!(plugin.components, vec!["skills"]);
    }

    #[test]
    fn validates_codex_manifest_and_multiple_component_paths() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".codex-plugin")).unwrap();
        std::fs::create_dir_all(temp.path().join("skills/demo")).unwrap();
        std::fs::write(temp.path().join("skills/demo/SKILL.md"), "# Demo").unwrap();
        std::fs::write(temp.path().join("mcp-one.json"), r#"{"mcpServers":{}}"#).unwrap();
        std::fs::write(temp.path().join("mcp-two.json"), r#"{"mcpServers":{}}"#).unwrap();
        std::fs::write(
            temp.path().join(".codex-plugin/plugin.json"),
            serde_json::json!({
                "name": "codex-compatible",
                "version": "1.0.0",
                "skills": "./skills/",
                "mcpServers": ["./mcp-one.json", "./mcp-two.json"],
                "apps": "./.app.json",
                "interface": {"displayName": "Codex Compatible"}
            })
            .to_string(),
        )
        .unwrap();

        let plugin = validate_plugin(temp.path()).unwrap();
        assert_eq!(plugin.manifest.name, "codex-compatible");
        assert_eq!(plugin.components, vec!["mcp", "skills"]);
        assert!(matches!(
            plugin.manifest.mcp_servers,
            Some(PluginPathOrInline::Paths(ref paths)) if paths.len() == 2
        ));
    }

    #[test]
    fn validates_javascript_sidebar_ui_component() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("ui")).unwrap();
        std::fs::write(
            temp.path().join("ui/sidebar.js"),
            concat!(
                "zode.ui.sidebar(() => ({ lines: [] }));",
                "zode.ui.statusLine(() => ({ spans: [] }));"
            ),
        )
        .unwrap();
        std::fs::write(
            temp.path().join("plugin.json"),
            serde_json::json!({
                "name": "sidebar-plugin",
                "ui": {
                    "sidebar": "./ui/sidebar.js",
                    "statusLine": "./ui/sidebar.js"
                }
            })
            .to_string(),
        )
        .unwrap();

        let plugin = validate_plugin(temp.path()).unwrap();
        assert_eq!(plugin.components, vec!["ui"]);
        assert_eq!(
            plugin
                .manifest
                .ui
                .as_ref()
                .and_then(|ui| ui.sidebar.as_deref()),
            Some("./ui/sidebar.js")
        );
        assert_eq!(
            plugin
                .manifest
                .ui
                .as_ref()
                .and_then(|ui| ui.status_line.as_deref()),
            Some("./ui/sidebar.js")
        );
    }

    #[test]
    fn honors_claude_default_enabled_on_first_install() {
        let home = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(source.path().join("skills/demo")).unwrap();
        std::fs::write(source.path().join("skills/demo/SKILL.md"), "# Demo").unwrap();
        std::fs::create_dir_all(source.path().join(".claude-plugin")).unwrap();
        std::fs::write(
            source.path().join(".claude-plugin/plugin.json"),
            serde_json::json!({
                "name": "opt-in-plugin",
                "defaultEnabled": false
            })
            .to_string(),
        )
        .unwrap();

        let manager = PluginPackageManager::at(home.path().to_path_buf()).unwrap();
        let installed = manager
            .install(source.path().to_str().unwrap(), home.path(), true)
            .unwrap();
        assert!(!installed.enabled);
        assert!(installed
            .path
            .starts_with(home.path().join("plugins-disabled")));
    }

    #[test]
    fn rejects_manifest_path_traversal() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("plugin.json"),
            r#"{"name":"bad-plugin","skills":"../outside"}"#,
        )
        .unwrap();
        assert!(validate_plugin(temp.path()).is_err());
    }

    #[test]
    fn local_install_update_and_uninstall_are_snapshotted() {
        let home = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_plugin(source.path(), "1.0.0");
        let manager = PluginPackageManager::at(home.path().to_path_buf()).unwrap();
        let installed = manager
            .install(source.path().to_str().unwrap(), home.path(), true)
            .unwrap();
        assert_eq!(installed.version.as_deref(), Some("1.0.0"));
        assert!(installed.path.join("skills/demo/SKILL.md").is_file());
        write_plugin(source.path(), "2.0.0");
        let updated = manager
            .update(Some("demo-plugin"), home.path(), false)
            .unwrap();
        assert_eq!(updated[0].version.as_deref(), Some("2.0.0"));
        manager.uninstall("demo-plugin", false).unwrap();
        assert!(!installed.path.exists());
    }

    #[test]
    fn update_refuses_broader_permissions_without_fresh_trust() {
        let home = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(source.path().join("skills/demo")).unwrap();
        std::fs::write(source.path().join("skills/demo/SKILL.md"), "# Demo").unwrap();
        let manifest = |permissions: serde_json::Value| {
            serde_json::json!({
                "name": "perm-plugin",
                "version": "1.0.0",
                "permissions": permissions
            })
            .to_string()
        };
        std::fs::write(
            source.path().join("plugin.json"),
            manifest(serde_json::json!({"network": ["api.example.com"]})),
        )
        .unwrap();
        let manager = PluginPackageManager::at(home.path().to_path_buf()).unwrap();
        let installed = manager
            .install(source.path().to_str().unwrap(), home.path(), true)
            .unwrap();
        assert_eq!(installed.permissions.network, vec!["api.example.com"]);

        // The source grows a new host and a secret env var…
        std::fs::write(
            source.path().join("plugin.json"),
            manifest(serde_json::json!({
                "network": ["api.example.com", "exfil.example.net"],
                "env": ["SOME_TOKEN"]
            })),
        )
        .unwrap();
        // …a plain update is refused…
        let error = manager
            .update(Some("perm-plugin"), home.path(), false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("broader permissions"), "{error}");
        // …and the installed snapshot keeps the old grant.
        assert_eq!(
            manager.registry().unwrap().plugins["perm-plugin"]
                .permissions
                .network,
            vec!["api.example.com"]
        );
        // Explicit re-trust accepts the wider grant.
        let updated = manager
            .update(Some("perm-plugin"), home.path(), true)
            .unwrap();
        assert_eq!(updated[0].permissions.env, vec!["SOME_TOKEN"]);
        // Shrinking or unchanged permissions update fine without re-trust.
        std::fs::write(
            source.path().join("plugin.json"),
            manifest(serde_json::json!({"network": ["api.example.com"]})),
        )
        .unwrap();
        assert!(manager
            .update(Some("perm-plugin"), home.path(), false)
            .is_ok());
    }

    #[test]
    fn install_requires_explicit_trust() {
        let home = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_plugin(source.path(), "1.0.0");
        let manager = PluginPackageManager::at(home.path().to_path_buf()).unwrap();
        assert!(manager
            .install(source.path().to_str().unwrap(), home.path(), false)
            .is_err());
    }
}
