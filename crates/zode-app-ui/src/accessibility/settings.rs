use accesskit::{Action, Role, Toggled};
use jian_core::CursorHint;
use zode_app_model::ZodeAppState;

use crate::{
    SettingsPanel, ThreadTranscript, WorkspaceLayout, ARCHIVED_TASK_FILTER_ID,
    ARCHIVED_TASK_SEARCH_ID, SETTINGS_BACK_ID, SETTINGS_SEARCH_ID,
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
