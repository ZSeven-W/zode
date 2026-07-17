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
    Inline(serde_json::Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub name: String,
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

impl PackageConfigComponent {
    /// Load the component's JSON: an inline value wins, else the referenced
    /// file is read and parsed. Failures warn (with the plugin name) and
    /// return None — a plugin's broken config must never break assembly, but
    /// it must not disappear silently either.
    pub fn load_json(&self) -> Option<serde_json::Value> {
        if let Some(value) = &self.inline {
            return Some(value.clone());
        }
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
            Ok(value) => Some(value),
            Err(error) => {
                tracing::warn!(plugin = %self.plugin, path = %path.display(),
                    "skip malformed plugin config: {error}");
                None
            }
        }
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
        .filter_map(|plugin| {
            let (configured, default) = match kind {
                PackageConfigKind::Hooks => (plugin.manifest.hooks.as_ref(), "hooks/hooks.json"),
                PackageConfigKind::Mcp => (plugin.manifest.mcp_servers.as_ref(), ".mcp.json"),
                PackageConfigKind::Lsp => (plugin.manifest.lsp_servers.as_ref(), ".lsp.json"),
            };
            let (path, inline) = match configured {
                Some(PluginPathOrInline::Path(relative)) => {
                    (Some(plugin.root.join(relative)), None)
                }
                Some(PluginPathOrInline::Inline(value)) => (None, Some(value.clone())),
                None => {
                    let path = plugin.root.join(default);
                    (path.is_file().then_some(path), None)
                }
            };
            (path.is_some() || inline.is_some()).then_some(PackageConfigComponent {
                plugin: plugin.manifest.name,
                root: plugin.root,
                path,
                inline,
            })
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
                "plugin install requires --trust because plugins may activate hooks, MCP servers, commands, and skills"
                    .into(),
            ));
        }
        if let Some(entry) = self.resolve_marketplace_plugin(source)? {
            return self.install_source(&entry.path.display().to_string(), cwd, trust, false);
        }
        self.install_source(source, cwd, trust, false)
    }

    pub fn update(
        &self,
        name: Option<&str>,
        cwd: &Path,
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
            updated.push(self.install_source(&target.source, cwd, true, true)?);
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
        let enabled = registry
            .plugins
            .get(&name)
            .is_none_or(|record| record.enabled);
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
        let updated = manager.update(Some("demo-plugin"), home.path()).unwrap();
        assert_eq!(updated[0].version.as_deref(), Some("2.0.0"));
        manager.uninstall("demo-plugin", false).unwrap();
        assert!(!installed.path.exists());
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
