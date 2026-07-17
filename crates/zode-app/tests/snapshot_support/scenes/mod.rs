mod conversation_artifacts;
mod conversation_document_preview;
mod conversation_environment;
mod conversation_queue;
mod empty_task;
mod integrations_catalog;
mod settings_general;

use std::collections::BTreeSet;

use zode_app_model::{ThemePreference, TranscriptVisualKind, ZodeAppState};

pub use conversation_artifacts::conversation_artifacts_scene;
pub use conversation_document_preview::conversation_document_preview_scene;
pub use conversation_environment::conversation_environment_scene;
pub use conversation_queue::conversation_queue_scene;
pub use empty_task::empty_task_scene;
pub use integrations_catalog::integrations_catalog_scene;
pub use settings_general::settings_general_scene;

pub const REFERENCE_SCENE_NAMES: [&str; 7] = [
    "empty-task",
    "integrations-catalog",
    "settings-general",
    "conversation-document-preview",
    "conversation-artifacts",
    "conversation-environment",
    "conversation-queue",
];

#[derive(Debug, Clone)]
pub struct ReferenceScene {
    pub name: &'static str,
    pub state: ZodeAppState,
}

impl ReferenceScene {
    pub fn block_count(&self) -> usize {
        self.state
            .current_session
            .as_ref()
            .and_then(|session| self.state.transcripts.get(session))
            .map_or(0, |transcript| transcript.items.len())
    }

    pub fn visual_kinds(&self) -> BTreeSet<TranscriptVisualKind> {
        self.state
            .current_session
            .as_ref()
            .and_then(|session| self.state.transcripts.get(session))
            .into_iter()
            .flat_map(|transcript| &transcript.items)
            .map(|item| item.visual_kind())
            .collect()
    }
}

pub const fn scene_names() -> [&'static str; 7] {
    REFERENCE_SCENE_NAMES
}

pub fn named_scene(
    name: &str,
    theme: ThemePreference,
    viewport_width: u32,
) -> Option<ReferenceScene> {
    match name {
        "empty-task" => Some(empty_task_scene(theme, viewport_width)),
        "integrations-catalog" => Some(integrations_catalog_scene(theme, viewport_width)),
        "settings-general" => Some(settings_general_scene(theme, viewport_width)),
        "conversation-document-preview" => {
            Some(conversation_document_preview_scene(theme, viewport_width))
        }
        "conversation-artifacts" => Some(conversation_artifacts_scene(theme, viewport_width)),
        "conversation-environment" => Some(conversation_environment_scene(theme, viewport_width)),
        "conversation-queue" => Some(conversation_queue_scene(theme, viewport_width)),
        _ => None,
    }
}

pub fn reference_scenes(theme: ThemePreference, viewport_width: u32) -> Vec<ReferenceScene> {
    REFERENCE_SCENE_NAMES
        .into_iter()
        .map(|name| {
            named_scene(name, theme, viewport_width)
                .expect("every registered reference scene has a builder")
        })
        .collect()
}
