use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    MarketplacePlugin, MarketplaceSource, PluginManifest, PluginPathOrInline, PluginPaths,
    ValidatedPlugin,
};
use crate::CoreError;

const MAX_PLUGIN_FILES: usize = 20_000;
const MAX_PLUGIN_BYTES: u64 = 256 * 1024 * 1024;
const MANIFEST_PATHS: &[&str] = &[
    "plugin.json",
    ".zode-plugin/plugin.json",
    ".codex-plugin/plugin.json",
    ".grok-plugin/plugin.json",
    ".claude-plugin/plugin.json",
];

#[derive(Debug)]
enum SourceKind {
    Local(PathBuf),
    Git(String),
}

#[derive(Debug)]
pub(super) struct SourceSpec {
    kind: SourceKind,
    pub(super) display_source: String,
    pub(super) git_ref: Option<String>,
    pub(super) subdir: Option<String>,
}

impl SourceSpec {
    pub(super) fn parse(input: &str, cwd: &Path) -> Result<Self, CoreError> {
        let (base, subdir) = input
            .split_once('#')
            .map_or((input, None), |(base, subdir)| {
                (base, Some(subdir.to_string()))
            });
        if let Some(subdir) = &subdir {
            validate_relative_path(subdir)?;
        }
        let candidate = if Path::new(base).is_absolute() {
            PathBuf::from(base)
        } else {
            cwd.join(base)
        };
        if candidate.is_dir() {
            let canonical = candidate.canonicalize()?;
            return Ok(Self {
                kind: SourceKind::Local(canonical.clone()),
                display_source: match &subdir {
                    Some(subdir) => format!("{}#{subdir}", canonical.display()),
                    None => canonical.display().to_string(),
                },
                git_ref: None,
                subdir,
            });
        }
        let shorthand =
            !base.contains("://") && !base.starts_with("git@") && base.matches('/').count() == 1;
        let (repo, git_ref) = if shorthand {
            base.rsplit_once('@')
                .map_or((base, None), |(repo, reference)| {
                    (repo, Some(reference.to_string()))
                })
        } else {
            (base, None)
        };
        let url = if shorthand {
            format!("https://github.com/{repo}.git")
        } else if base.contains("://") || base.starts_with("git@") {
            base.to_string()
        } else {
            return Err(CoreError::Other(format!(
                "plugin source not found: {input}"
            )));
        };
        Ok(Self {
            kind: SourceKind::Git(url.clone()),
            display_source: input.to_string(),
            git_ref,
            subdir,
        })
    }
}

pub(super) fn materialize_source(spec: &SourceSpec, destination: &Path) -> Result<(), CoreError> {
    let parent = destination
        .parent()
        .ok_or_else(|| CoreError::Other("source destination has no parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let stage = parent.join(format!(".source-{}", Uuid::new_v4().simple()));
    match &spec.kind {
        SourceKind::Local(path) => copy_dir_snapshot(path, &stage)?,
        SourceKind::Git(url) => {
            let mut command = Command::new("git");
            command.arg("clone").arg("--depth=1");
            if let Some(reference) = &spec.git_ref {
                command.arg("--branch").arg(reference);
            }
            let output = command.arg("--").arg(url).arg(&stage).output()?;
            if !output.status.success() {
                cleanup_dir(&stage);
                return Err(CoreError::Other(format!(
                    "git clone failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )));
            }
            cleanup_dir(&stage.join(".git"));
        }
    }
    if destination.exists() {
        let backup = parent.join(format!(".old-source-{}", Uuid::new_v4().simple()));
        std::fs::rename(destination, &backup)?;
        match std::fs::rename(&stage, destination) {
            Ok(()) => cleanup_dir(&backup),
            Err(error) => {
                let _ = std::fs::rename(&backup, destination);
                cleanup_dir(&stage);
                return Err(error.into());
            }
        }
    } else {
        std::fs::rename(stage, destination)?;
    }
    Ok(())
}

pub(super) fn locate_plugin_root(fetch: &Path, subdir: Option<&str>) -> Result<PathBuf, CoreError> {
    if let Some(subdir) = subdir {
        let root = fetch.join(subdir);
        ensure_contained(fetch, &root)?;
        return Ok(root);
    }
    if validate_plugin(fetch).is_ok() {
        return Ok(fetch.to_path_buf());
    }
    let plugins = fetch.join("plugins");
    let candidates: Vec<PathBuf> = std::fs::read_dir(&plugins)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && validate_plugin(path).is_ok())
        .collect();
    match candidates.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(CoreError::Other(
            "no plugin found (expected plugin.json or convention components)".into(),
        )),
        _ => Err(CoreError::Other(
            "source contains multiple plugins; select one with #subdir".into(),
        )),
    }
}

pub fn validate_plugin(root: &Path) -> Result<ValidatedPlugin, CoreError> {
    if !root.is_dir() {
        return Err(CoreError::Other(format!(
            "plugin directory not found: {}",
            root.display()
        )));
    }
    let manifest_path = MANIFEST_PATHS
        .iter()
        .map(|relative| root.join(relative))
        .find(|path| path.is_file());
    let manifest = if let Some(path) = &manifest_path {
        serde_json::from_slice::<PluginManifest>(&std::fs::read(path)?)?
    } else {
        PluginManifest {
            name: derived_name(root)?,
            default_enabled: None,
            version: None,
            description: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: Vec::new(),
            skills: None,
            commands: None,
            agents: None,
            hooks: None,
            mcp_servers: None,
            lsp_servers: None,
            ui: None,
            permissions: Default::default(),
        }
    };
    validate_name(&manifest.name)?;
    let mut components = Vec::new();
    validate_paths(root, "skills", manifest.skills.as_ref(), &mut components)?;
    validate_paths(
        root,
        "commands",
        manifest.commands.as_ref(),
        &mut components,
    )?;
    validate_paths(root, "agents", manifest.agents.as_ref(), &mut components)?;
    validate_component(
        root,
        "hooks",
        "hooks/hooks.json",
        manifest.hooks.as_ref(),
        &mut components,
    )?;
    validate_component(
        root,
        "mcp",
        ".mcp.json",
        manifest.mcp_servers.as_ref(),
        &mut components,
    )?;
    validate_component(
        root,
        "lsp",
        ".lsp.json",
        manifest.lsp_servers.as_ref(),
        &mut components,
    )?;
    if let Some(relative) = manifest.ui.as_ref().and_then(|ui| ui.sidebar.as_deref()) {
        validate_script_path(root, "ui.sidebar", relative)?;
        components.push("ui".to_string());
    }
    if let Some(relative) = manifest
        .ui
        .as_ref()
        .and_then(|ui| ui.status_line.as_deref())
    {
        validate_script_path(root, "ui.statusLine", relative)?;
        components.push("ui".to_string());
    }
    validate_permissions(&manifest.permissions)?;
    components.sort();
    components.dedup();
    if components.is_empty() {
        return Err(CoreError::Other(
            "plugin has no discoverable skills, commands, agents, hooks, MCP, LSP, or UI components"
                .into(),
        ));
    }
    Ok(ValidatedPlugin {
        root: root.to_path_buf(),
        manifest_path,
        manifest,
        components,
    })
}

fn validate_permissions(permissions: &super::PluginPermissions) -> Result<(), CoreError> {
    for pattern in &permissions.network {
        let host = pattern.strip_prefix("*.").unwrap_or(pattern);
        if host.is_empty()
            || host.contains('/')
            || host.contains(':')
            || host.starts_with('.')
            || host.ends_with('.')
            || !host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
        {
            return Err(CoreError::Other(format!(
                "invalid plugin network hostname permission: {pattern}"
            )));
        }
    }
    for name in &permissions.env {
        let mut bytes = name.bytes();
        let valid_first = bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_');
        if !valid_first || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
            return Err(CoreError::Other(format!(
                "invalid plugin environment permission: {name}"
            )));
        }
    }
    const CONTEXT_SCOPES: &[&str] = &["tabs", "workspace", "tools", "tasks", "services"];
    for scope in &permissions.context {
        if !CONTEXT_SCOPES.contains(&scope.as_str()) {
            return Err(CoreError::Other(format!(
                "invalid plugin context permission: {scope}"
            )));
        }
    }
    Ok(())
}

fn validate_script_path(root: &Path, label: &str, relative: &str) -> Result<(), CoreError> {
    validate_relative_path(relative)?;
    let path = root.join(relative);
    ensure_contained(root, &path)?;
    if !path.is_file() {
        return Err(CoreError::Other(format!(
            "plugin {label} path does not exist: {relative}"
        )));
    }
    if path.extension().and_then(|value| value.to_str()) != Some("js") {
        return Err(CoreError::Other(format!(
            "plugin {label} path must be a .js file: {relative}"
        )));
    }
    let size = std::fs::metadata(&path)?.len();
    if size > crate::ui_extensions::MAX_UI_SCRIPT_BYTES as u64 {
        return Err(CoreError::Other(format!(
            "plugin {label} script exceeds {} bytes: {relative}",
            crate::ui_extensions::MAX_UI_SCRIPT_BYTES
        )));
    }
    Ok(())
}

fn validate_paths(
    root: &Path,
    label: &str,
    configured: Option<&PluginPaths>,
    components: &mut Vec<String>,
) -> Result<(), CoreError> {
    let paths: Vec<&str> = configured
        .map(PluginPaths::values)
        .unwrap_or_else(|| vec![label]);
    for relative in paths {
        validate_relative_path(relative)?;
        let path = root.join(relative);
        if !path.exists() {
            if configured.is_some() {
                return Err(CoreError::Other(format!(
                    "plugin {label} path does not exist: {relative}"
                )));
            }
            continue;
        }
        ensure_contained(root, &path)?;
        if path.is_dir() {
            components.push(label.to_string());
        } else {
            return Err(CoreError::Other(format!(
                "plugin {label} path is not a directory: {relative}"
            )));
        }
    }
    Ok(())
}

fn validate_component(
    root: &Path,
    label: &str,
    default: &str,
    configured: Option<&PluginPathOrInline>,
    components: &mut Vec<String>,
) -> Result<(), CoreError> {
    match configured {
        Some(PluginPathOrInline::Inline(value)) => {
            if !value.is_object() && !value.is_array() {
                return Err(CoreError::Other(format!(
                    "inline {label} component must be an object or array"
                )));
            }
            components.push(label.to_string());
        }
        Some(PluginPathOrInline::Path(relative)) => {
            validate_config_path(root, label, relative)?;
            components.push(label.to_string());
        }
        Some(PluginPathOrInline::Paths(relative)) => {
            if relative.is_empty() {
                return Err(CoreError::Other(format!(
                    "plugin {label} paths must not be empty"
                )));
            }
            for relative in relative {
                validate_config_path(root, label, relative)?;
            }
            components.push(label.to_string());
        }
        None => {
            let path = root.join(default);
            if path.is_file() {
                parse_json_file(&path)?;
                components.push(label.to_string());
            }
        }
    }
    Ok(())
}

fn validate_config_path(root: &Path, label: &str, relative: &str) -> Result<(), CoreError> {
    validate_relative_path(relative)?;
    let path = root.join(relative);
    ensure_contained(root, &path)?;
    if !path.is_file() {
        return Err(CoreError::Other(format!(
            "plugin {label} path does not exist: {relative}"
        )));
    }
    parse_json_file(&path)
}

pub(super) fn scan_marketplace(
    source: &MarketplaceSource,
) -> Result<Vec<MarketplacePlugin>, CoreError> {
    #[derive(Deserialize)]
    struct StaticIndex {
        #[serde(default)]
        plugins: Vec<StaticEntry>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StaticEntry {
        name: String,
        path: String,
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        description: Option<String>,
    }
    let index_path = source.cache_path.join("plugin-index.json");
    let candidates: Vec<(PathBuf, Option<StaticEntry>)> = if index_path.is_file() {
        let index: StaticIndex = serde_json::from_slice(&std::fs::read(index_path)?)?;
        index
            .plugins
            .into_iter()
            .map(|entry| (source.cache_path.join(&entry.path), Some(entry)))
            .collect()
    } else {
        let plugins_root = source.cache_path.join("plugins");
        std::fs::read_dir(plugins_root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| (entry.path(), None))
            .filter(|(path, _)| path.is_dir())
            .collect()
    };
    let mut output = Vec::new();
    for (path, indexed) in candidates {
        ensure_contained(&source.cache_path, &path)?;
        let validated = validate_plugin(&path)?;
        if let Some(indexed) = &indexed {
            if indexed.name != validated.manifest.name {
                return Err(CoreError::Other(format!(
                    "marketplace index name {} does not match manifest name {}",
                    indexed.name, validated.manifest.name
                )));
            }
        }
        output.push(MarketplacePlugin {
            marketplace: source.name.clone(),
            name: validated.manifest.name,
            version: indexed
                .as_ref()
                .and_then(|entry| entry.version.clone())
                .or(validated.manifest.version),
            description: indexed
                .as_ref()
                .and_then(|entry| entry.description.clone())
                .or(validated.manifest.description),
            path,
            components: validated.components,
        });
    }
    Ok(output)
}

fn parse_json_file(path: &Path) -> Result<(), CoreError> {
    let _: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    Ok(())
}

pub(super) fn validate_name(name: &str) -> Result<(), CoreError> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(CoreError::Other(format!(
            "invalid plugin name {name:?}; expected 1-64 lowercase letters, digits, or hyphens"
        )))
    }
}

fn derived_name(root: &Path) -> Result<String, CoreError> {
    let raw = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CoreError::Other("plugin directory has no UTF-8 name".into()))?;
    let derived = raw
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    let derived = derived.trim_matches('-').to_string();
    validate_name(&derived)?;
    Ok(derived)
}

fn validate_relative_path(path: &str) -> Result<(), CoreError> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(CoreError::Other(format!(
            "plugin path must stay relative to its root: {}",
            path.display()
        )));
    }
    Ok(())
}

fn ensure_contained(root: &Path, path: &Path) -> Result<(), CoreError> {
    let root = root.canonicalize()?;
    let path = path.canonicalize()?;
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(CoreError::Other(format!(
            "plugin path escapes its root: {}",
            path.display()
        )))
    }
}

pub(super) fn copy_dir_snapshot(source: &Path, destination: &Path) -> Result<(), CoreError> {
    let mut count = 0usize;
    let mut bytes = 0u64;
    copy_dir_inner(source, destination, &mut count, &mut bytes)
}

fn copy_dir_inner(
    source: &Path,
    destination: &Path,
    count: &mut usize,
    bytes: &mut u64,
) -> Result<(), CoreError> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_name() == ".git" {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(CoreError::Other(format!(
                "plugin snapshots reject symlinks: {}",
                entry.path().display()
            )));
        }
        *count += 1;
        if *count > MAX_PLUGIN_FILES {
            return Err(CoreError::Other(format!(
                "plugin exceeds {MAX_PLUGIN_FILES} files"
            )));
        }
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_dir_inner(&entry.path(), &target, count, bytes)?;
        } else if kind.is_file() {
            let size = entry.metadata()?.len();
            *bytes = bytes.saturating_add(size);
            if *bytes > MAX_PLUGIN_BYTES {
                return Err(CoreError::Other(format!(
                    "plugin exceeds {} MiB",
                    MAX_PLUGIN_BYTES / 1024 / 1024
                )));
            }
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

pub(super) fn hash_tree(root: &Path) -> Result<String, CoreError> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut digest = Sha256::new();
    for (relative, path) in files {
        digest.update(relative.as_bytes());
        digest.update([0]);
        digest.update(std::fs::read(path)?);
        digest.update([0]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn collect_files(
    root: &Path,
    current: &Path,
    output: &mut Vec<(String, PathBuf)>,
) -> Result<(), CoreError> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            collect_files(root, &entry.path(), output)?;
        } else if entry.file_type()?.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|error| CoreError::Other(error.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            output.push((relative, entry.path()));
        }
    }
    Ok(())
}

pub(super) fn marketplace_name(source: &str) -> String {
    let tail = source
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("marketplace")
        .trim_end_matches(".git");
    let base = tail
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    let base = base.trim_matches('-');
    let hash = Sha256::digest(source.as_bytes());
    format!("{}-{:02x}{:02x}{:02x}", base, hash[0], hash[1], hash[2])
}

pub(super) fn read_json_or_default<T>(path: &Path) -> Result<T, CoreError>
where
    T: serde::de::DeserializeOwned + Default,
{
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn cleanup_dir(path: &Path) {
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    }
}

pub(super) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
