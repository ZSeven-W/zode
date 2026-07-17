use accesskit::{Action, Role, Toggled};
use jian_core::CursorHint;
use zode_app_model::ZodeAppState;

use crate::{
    SettingsPanel, ThreadTranscript, WorkspaceLayout, ARCHIVED_TASK_FILTER_ID,
    ARCHIVED_TASK_SEARCH_ID, COMPUTER_ALLOWED_APP_ADD_ID, COMPUTER_ALLOWED_APP_INPUT_ID,
    PROVIDER_ADD_ID, PROVIDER_API_KEY_INPUT_ID, PROVIDER_CANCEL_ID, PROVIDER_SAVE_ID,
    SETTINGS_BACK_ID, SETTINGS_SEARCH_ID,
};

use super::{
    next_order, node, visible_rect, InteractionNode, SETTINGS_ROOT_ID, SIDEBAR_ID, THEME_DARK_ID,
    THEME_LIGHT_ID, THEME_SYSTEM_ID,
};

pub(super) fn append_settings_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let settings = SettingsPanel::layout(layout.sidebar, layout.primary_surface, state);
    if visible_rect(layout.sidebar) {
        nodes.push(node(
            SIDEBAR_ID,
            layout.sidebar,
            Role::Navigation,
            "设置分类",
            None,
            Vec::new(),
            None,
            CursorHint::Default,
        ));
        nodes.push(node(
            SETTINGS_BACK_ID,
            settings.navigation.title,
            Role::Button,
            "返回应用",
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
        nodes.push(node(
            SETTINGS_SEARCH_ID,
            settings.navigation.search,
            Role::SearchInput,
            "搜索设置",
            Some(state.settings_search.clone()),
            vec![Action::Focus, Action::SetValue],
            next_order(focus_order),
            CursorHint::Text,
        ));
        for entry in &settings.navigation.entries {
            let Some(visible) = ThreadTranscript::clip_to_viewport(entry.rect, layout.sidebar)
            else {
                continue;
            };
            let mut category = node(
                entry.id,
                visible,
                Role::Button,
                entry.label,
                None,
                if entry.enabled {
                    vec![Action::Click, Action::Focus]
                } else {
                    Vec::new()
                },
                entry.enabled.then(|| next_order(focus_order)).flatten(),
                if entry.enabled {
                    CursorHint::Pointer
                } else {
                    CursorHint::NotAllowed
                },
            );
            category.toggled = Some(Toggled::from(entry.selected));
            category.disabled = !entry.enabled;
            nodes.push(category);
        }
    }
    let content = settings.content;
    if visible_rect(content) {
        let max_scroll = SettingsPanel::max_scroll_offset(content, state);
        let mut actions = Vec::new();
        if settings.scroll_offset > 0.0 {
            actions.push(Action::ScrollUp);
        }
        if settings.scroll_offset < max_scroll {
            actions.push(Action::ScrollDown);
        }
        nodes.push(node(
            SETTINGS_ROOT_ID,
            content,
            Role::ScrollView,
            "设置内容",
            None,
            actions,
            None,
            CursorHint::Default,
        ));
    }

    if SettingsPanel::active_category(state) == zode_app_model::SettingsCategory::General {
        for preset in &settings.general.permission_presets {
            let Some(visible_rect) = preset.visible_rect else {
                continue;
            };
            let mut control = node(
                preset.id,
                visible_rect,
                Role::RadioButton,
                preset.label,
                Some(preset.description.into()),
                if preset.enabled {
                    vec![Action::Click, Action::Focus]
                } else {
                    Vec::new()
                },
                preset.enabled.then(|| next_order(focus_order)).flatten(),
                if preset.enabled {
                    CursorHint::Pointer
                } else {
                    CursorHint::NotAllowed
                },
            );
            control.toggled = Some(Toggled::from(preset.selected));
            control.disabled = !preset.enabled;
            nodes.push(control);
        }
        for row in &settings.general.general_rows {
            let Some(visible_rect) = row.visible_rect else {
                continue;
            };
            let mut setting = node(
                row.id,
                visible_rect,
                if row.toggled.is_some() {
                    Role::Switch
                } else {
                    Role::Button
                },
                row.label,
                Some(row.value.clone()),
                if row.enabled {
                    vec![Action::Click, Action::Focus]
                } else {
                    Vec::new()
                },
                row.enabled.then(|| next_order(focus_order)).flatten(),
                if row.enabled {
                    CursorHint::Pointer
                } else {
                    CursorHint::NotAllowed
                },
            );
            setting.toggled = row.toggled.map(Toggled::from);
            setting.disabled = !row.enabled;
            nodes.push(setting);
        }
    }

    for control_layout in SettingsPanel::appearance_control_layout(content, state) {
        let role = if matches!(
            control_layout.id,
            THEME_SYSTEM_ID | THEME_LIGHT_ID | THEME_DARK_ID
        ) {
            Role::RadioButton
        } else {
            Role::Switch
        };
        let mut control = node(
            control_layout.id,
            control_layout.visible_rect,
            role,
            &control_layout.control.label,
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        );
        control.toggled = Some(if control_layout.control.selected {
            Toggled::True
        } else {
            Toggled::False
        });
        nodes.push(control);
    }

    if SettingsPanel::active_category(state) == zode_app_model::SettingsCategory::ProviderModels {
        append_provider_model_nodes(
            nodes,
            focus_order,
            state,
            content,
            &settings.provider_models,
        );
    }

    if SettingsPanel::active_category(state) == zode_app_model::SettingsCategory::ComputerUse {
        append_computer_use_nodes(nodes, focus_order, state, content, &settings.computer_use);
    }

    if SettingsPanel::active_category(state) == zode_app_model::SettingsCategory::ArchivedTasks {
        if let Some(search_rect) =
            ThreadTranscript::clip_to_viewport(settings.archived.search_rect, content)
        {
            nodes.push(node(
                ARCHIVED_TASK_SEARCH_ID,
                search_rect,
                Role::SearchInput,
                "搜索已归档任务",
                Some(state.archived_tasks.search.clone()),
                vec![Action::Focus, Action::SetValue],
                next_order(focus_order),
                CursorHint::Text,
            ));
        }
        if let Some(filter_rect) =
            ThreadTranscript::clip_to_viewport(settings.archived.filter_rect, content)
        {
            let mut filter = node(
                ARCHIVED_TASK_FILTER_ID,
                filter_rect,
                Role::Button,
                "筛选归档任务项目",
                Some(settings.archived.filter_label.clone()),
                if settings.archived.filter_enabled {
                    vec![Action::Click, Action::Focus]
                } else {
                    Vec::new()
                },
                settings
                    .archived
                    .filter_enabled
                    .then(|| next_order(focus_order))
                    .flatten(),
                if settings.archived.filter_enabled {
                    CursorHint::Pointer
                } else {
                    CursorHint::NotAllowed
                },
            );
            filter.disabled = !settings.archived.filter_enabled;
            nodes.push(filter);
        }
        for row in settings
            .archived
            .groups
            .iter()
            .flat_map(|group| group.rows.iter())
        {
            let Some(visible_rect) = row.visible_action_rect else {
                continue;
            };
            nodes.push(node(
                row.id,
                visible_rect,
                Role::Button,
                &format!("取消归档 {}", row.title),
                Some(row.workspace_uri.as_str().into()),
                vec![Action::Click, Action::Focus],
                next_order(focus_order),
                CursorHint::Pointer,
            ));
        }
    }

    let Some(workspace_uri) = SettingsPanel::active_workspace_uri(state) else {
        return;
    };
    for row in SettingsPanel::permission_row_layout(content, state, workspace_uri) {
        nodes.push(node(
            row.id,
            row.visible_rect,
            Role::Button,
            &format!("撤销 {} 权限", row.tool),
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
}

fn append_provider_model_nodes(
    nodes: &mut Vec<InteractionNode>,
    focus_order: &mut u32,
    state: &ZodeAppState,
    viewport: jian_widgets::Rect,
    layout: &crate::ProviderModelsLayout,
) {
    let busy = state.provider_models.status.is_busy();
    if let Some(rect) = ThreadTranscript::clip_to_viewport(layout.add_rect, viewport) {
        let mut add = node(
            PROVIDER_ADD_ID,
            rect,
            Role::Button,
            "添加 Provider",
            None,
            if busy {
                Vec::new()
            } else {
                vec![Action::Click, Action::Focus]
            },
            (!busy).then(|| next_order(focus_order)).flatten(),
            if busy {
                CursorHint::NotAllowed
            } else {
                CursorHint::Pointer
            },
        );
        add.disabled = busy;
        nodes.push(add);
    }
    for provider in &layout.providers {
        if let Some(rect) = ThreadTranscript::clip_to_viewport(provider.edit_rect, viewport) {
            nodes.push(node(
                provider.id,
                rect,
                Role::Button,
                &format!("编辑 Provider {}", provider.summary.provider_id),
                Some(format!(
                    "{}，{} 个模型",
                    provider_kind_label(provider.summary.kind),
                    provider.summary.models.len()
                )),
                vec![Action::Click, Action::Focus],
                next_order(focus_order),
                CursorHint::Pointer,
            ));
        }
        if let Some(rect) = ThreadTranscript::clip_to_viewport(provider.remove_rect, viewport) {
            let mut remove = node(
                provider.remove_id,
                rect,
                Role::Button,
                &format!("删除 Provider {}", provider.summary.provider_id),
                None,
                if busy {
                    Vec::new()
                } else {
                    vec![Action::Click, Action::Focus]
                },
                (!busy).then(|| next_order(focus_order)).flatten(),
                if busy {
                    CursorHint::NotAllowed
                } else {
                    CursorHint::Pointer
                },
            );
            remove.disabled = busy;
            nodes.push(remove);
        }
    }
    let Some(editor) = layout.editor.as_ref() else {
        return;
    };
    for field in &editor.fields {
        if field.label == "类型" {
            continue;
        }
        let Some(rect) = ThreadTranscript::clip_to_viewport(field.rect, viewport) else {
            continue;
        };
        let secret = field.id == PROVIDER_API_KEY_INPUT_ID;
        nodes.push(node(
            field.id,
            rect,
            Role::TextInput,
            field.label,
            (!secret).then(|| field.value.clone()),
            vec![Action::Focus, Action::SetValue],
            next_order(focus_order),
            CursorHint::Text,
        ));
    }
    for kind in &editor.kinds {
        let Some(rect) = ThreadTranscript::clip_to_viewport(kind.rect, viewport) else {
            continue;
        };
        let mut option = node(
            kind.id,
            rect,
            Role::RadioButton,
            kind.label,
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        );
        option.toggled = Some(Toggled::from(kind.selected));
        nodes.push(option);
    }
    for model in &editor.models {
        let Some(rect) = ThreadTranscript::clip_to_viewport(model.rect, viewport) else {
            continue;
        };
        let mut option = node(
            model.id,
            rect,
            Role::RadioButton,
            &format!("设为新会话默认模型 {}", model.model_id),
            Some(model.model_id.clone()),
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        );
        option.toggled = Some(Toggled::from(model.selected));
        nodes.push(option);
    }
    for (id, rect, name) in [
        (PROVIDER_CANCEL_ID, editor.cancel_rect, "取消编辑"),
        (PROVIDER_SAVE_ID, editor.save_rect, "保存 Provider"),
    ] {
        let Some(rect) = ThreadTranscript::clip_to_viewport(rect, viewport) else {
            continue;
        };
        let mut action = node(
            id,
            rect,
            Role::Button,
            name,
            None,
            if busy {
                Vec::new()
            } else {
                vec![Action::Click, Action::Focus]
            },
            (!busy).then(|| next_order(focus_order)).flatten(),
            if busy {
                CursorHint::NotAllowed
            } else {
                CursorHint::Pointer
            },
        );
        action.disabled = busy;
        nodes.push(action);
    }
}

fn append_computer_use_nodes(
    nodes: &mut Vec<InteractionNode>,
    focus_order: &mut u32,
    state: &ZodeAppState,
    viewport: jian_widgets::Rect,
    layout: &crate::ComputerUseLayout,
) {
    for row in &layout.permission_rows {
        if !row.actionable {
            continue;
        }
        let Some(rect) = ThreadTranscript::clip_to_viewport(row.action_rect, viewport) else {
            continue;
        };
        nodes.push(node(
            row.action_id,
            rect,
            Role::Button,
            &format!("打开系统设置：{}", row.label),
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
    for row in &layout.access_rows {
        let Some(rect) = row.visible_rect else {
            continue;
        };
        let mut setting = node(
            row.id,
            rect,
            if row.toggled.is_some() {
                Role::Switch
            } else {
                Role::Button
            },
            row.label,
            Some(row.value.clone()),
            if row.enabled {
                vec![Action::Click, Action::Focus]
            } else {
                Vec::new()
            },
            row.enabled.then(|| next_order(focus_order)).flatten(),
            if row.enabled {
                CursorHint::Pointer
            } else {
                CursorHint::NotAllowed
            },
        );
        setting.toggled = row.toggled.map(Toggled::from);
        setting.disabled = !row.enabled;
        nodes.push(setting);
    }
    for row in &layout.allowed_app_rows {
        let Some(rect) = ThreadTranscript::clip_to_viewport(row.remove_rect, viewport) else {
            continue;
        };
        nodes.push(node(
            row.id,
            rect,
            Role::Button,
            &format!("移除允许的应用 {}", row.app),
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
    if let Some(rect) = ThreadTranscript::clip_to_viewport(layout.allowed_app_input_rect, viewport)
    {
        nodes.push(node(
            COMPUTER_ALLOWED_APP_INPUT_ID,
            rect,
            Role::TextInput,
            "应用名称",
            Some(state.computer_use.allowed_app_input.clone()),
            vec![Action::Focus, Action::SetValue],
            next_order(focus_order),
            CursorHint::Text,
        ));
    }
    if let Some(rect) = ThreadTranscript::clip_to_viewport(layout.allowed_app_add_rect, viewport) {
        let enabled = !state.computer_use.allowed_app_input.trim().is_empty();
        let mut add = node(
            COMPUTER_ALLOWED_APP_ADD_ID,
            rect,
            Role::Button,
            "添加允许的应用",
            None,
            if enabled {
                vec![Action::Click, Action::Focus]
            } else {
                Vec::new()
            },
            enabled.then(|| next_order(focus_order)).flatten(),
            if enabled {
                CursorHint::Pointer
            } else {
                CursorHint::NotAllowed
            },
        );
        add.disabled = !enabled;
        nodes.push(add);
    }
}

fn provider_kind_label(kind: zode_app_model::ProviderKindChoice) -> &'static str {
    match kind {
        zode_app_model::ProviderKindChoice::Anthropic => "Anthropic",
        zode_app_model::ProviderKindChoice::OpenAi => "OpenAI",
        zode_app_model::ProviderKindChoice::Ollama => "Ollama",
    }
}
