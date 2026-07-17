use std::collections::BTreeMap;

use zode_node_protocol::{
    IntegrationRegistryEntry, IntegrationRegistryKind, IntegrationRegistrySnapshot,
    IntegrationRegistryState, WorkspaceUri,
};

/// Stable catalog categories sourced from local registries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IntegrationCategory {
    BuiltInTools,
    Skills,
    Mcp,
    Lsp,
    Capabilities,
}

impl IntegrationCategory {
    pub const fn title(self) -> &'static str {
        match self {
            Self::BuiltInTools => "内置工具",
            Self::Skills => "技能",
            Self::Mcp => "MCP 服务",
            Self::Lsp => "语言服务",
            Self::Capabilities => "节点能力",
        }
    }
}

/// A status the runtime can prove without launching an integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Ready,
    Configured,
    Disabled,
}

impl Availability {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "可用",
            Self::Configured => "已配置",
            Self::Disabled => "已停用",
        }
    }
}

/// How an integration became present on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationInstallState {
    BuiltIn,
    Installed,
    Configured,
    /// A verified directory offer that is not installed yet. The local
    /// registry never manufactures this state when no directory is available.
    Available,
}

impl IntegrationInstallState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::BuiltIn => "已内置",
            Self::Installed => "已安装",
            Self::Configured => "已配置",
            Self::Available => "可安装",
        }
    }
}

/// In-flight mutation state for a locally discovered plugin entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum IntegrationMutationState {
    #[default]
    Idle,
    Updating {
        source_id: String,
        enabled: bool,
    },
    Failed {
        source_id: String,
        message: String,
    },
}

/// Auditable icon fallback. Repository-backed branded assets can be added as
/// another enum variant; missing assets always use a non-empty monogram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationIcon {
    Monogram(String),
}

impl IntegrationIcon {
    pub fn label(&self) -> &str {
        match self {
            Self::Monogram(label) => label,
        }
    }
}

/// One catalog row projected from exactly one runtime registry source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationEntry {
    pub source_id: Option<String>,
    pub name: String,
    pub description: String,
    pub category: IntegrationCategory,
    pub installed: bool,
    pub availability: Availability,
    pub install_state: IntegrationInstallState,
    pub icon: IntegrationIcon,
    /// Reserved for test scene builders. Production projection always writes
    /// false, which makes fixture leakage directly testable.
    pub fixture_only: bool,
}

/// Compact icon-band item derived from an installed catalog row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledIntegration {
    pub source_id: Option<String>,
    pub name: String,
    pub icon: IntegrationIcon,
    pub availability: Availability,
}

/// One non-empty catalog section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationSection {
    pub category: IntegrationCategory,
    pub rows: Vec<IntegrationEntry>,
}

/// Presentation-ready catalog that never depends on a network marketplace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationCatalog {
    pub workspace_uri: WorkspaceUri,
    pub installed: Vec<InstalledIntegration>,
    pub sections: Vec<IntegrationSection>,
    pub directory_error: Option<String>,
}

impl IntegrationCatalog {
    pub fn all_entries(&self) -> impl Iterator<Item = &IntegrationEntry> {
        self.sections.iter().flat_map(|section| section.rows.iter())
    }
}

/// Converts a typed runtime registry snapshot into production presentation
/// state. The mapper deliberately owns `fixture_only = false`.
pub fn integration_catalog(snapshot: IntegrationRegistrySnapshot) -> IntegrationCatalog {
    let mut grouped = BTreeMap::<IntegrationCategory, Vec<IntegrationEntry>>::new();
    for source in snapshot.entries {
        let entry = integration_entry(source);
        grouped.entry(entry.category).or_default().push(entry);
    }
    for rows in grouped.values_mut() {
        rows.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.source_id.cmp(&right.source_id))
        });
    }

    let sections = [
        IntegrationCategory::BuiltInTools,
        IntegrationCategory::Capabilities,
        IntegrationCategory::Skills,
        IntegrationCategory::Mcp,
        IntegrationCategory::Lsp,
    ]
    .into_iter()
    .filter_map(|category| {
        grouped
            .remove(&category)
            .filter(|rows| !rows.is_empty())
            .map(|rows| IntegrationSection { category, rows })
    })
    .collect::<Vec<_>>();
    let installed = sections
        .iter()
        .flat_map(|section| section.rows.iter())
        .filter(|entry| entry.installed)
        .map(|entry| InstalledIntegration {
            source_id: entry.source_id.clone(),
            name: entry.name.clone(),
            icon: entry.icon.clone(),
            availability: entry.availability,
        })
        .collect();

    IntegrationCatalog {
        workspace_uri: snapshot.workspace_uri,
        installed,
        sections,
        directory_error: snapshot.directory_error,
    }
}

fn integration_entry(source: IntegrationRegistryEntry) -> IntegrationEntry {
    let category = match source.kind {
        IntegrationRegistryKind::ToolGroup => IntegrationCategory::BuiltInTools,
        IntegrationRegistryKind::Skill => IntegrationCategory::Skills,
        IntegrationRegistryKind::Mcp => IntegrationCategory::Mcp,
        IntegrationRegistryKind::Lsp => IntegrationCategory::Lsp,
        IntegrationRegistryKind::NodeCapability => IntegrationCategory::Capabilities,
    };
    let availability = match source.state {
        IntegrationRegistryState::Ready => Availability::Ready,
        IntegrationRegistryState::Configured => Availability::Configured,
        IntegrationRegistryState::Disabled => Availability::Disabled,
    };
    let install_state = match category {
        IntegrationCategory::BuiltInTools | IntegrationCategory::Capabilities => {
            IntegrationInstallState::BuiltIn
        }
        IntegrationCategory::Skills => IntegrationInstallState::Installed,
        IntegrationCategory::Mcp | IntegrationCategory::Lsp => IntegrationInstallState::Configured,
    };
    let icon = IntegrationIcon::Monogram(stable_monogram(&source.name));
    IntegrationEntry {
        source_id: Some(source.source_id),
        name: source.name,
        description: source.description,
        category,
        installed: source.installed,
        availability,
        install_state,
        icon,
        fixture_only: false,
    }
}

fn stable_monogram(name: &str) -> String {
    let words = name
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let mut label = if words.len() >= 2 {
        words
            .iter()
            .take(2)
            .filter_map(|word| word.chars().next())
            .collect::<String>()
    } else {
        words
            .first()
            .copied()
            .unwrap_or(name)
            .chars()
            .take(2)
            .collect::<String>()
    };
    if label.is_empty() {
        label.push('Z');
    }
    label.to_uppercase()
}
