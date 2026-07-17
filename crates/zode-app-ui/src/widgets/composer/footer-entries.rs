use jian_widgets::Rect;
use zode_app_model::{ComposerFooterMenu, ZodeAppState};
use zode_node_protocol::{ApprovalMode, SandboxMode};

use super::{
    effort_label, runtime_controls_locked, ComposerFooterRowLayout, COMPACT_ROW_H,
    COMPOSER_ADD_FILE_ID, COMPOSER_ADD_GOAL_ID, COMPOSER_ADD_PLAN_ID, COMPOSER_ADD_WECHAT_ID,
    COMPOSER_MODEL_BACK_ID, COMPOSER_MODEL_CONFIGURE_ID, COMPOSER_MODEL_EFFORTS_ID,
    COMPOSER_MODEL_EFFORT_HIGH_ID, COMPOSER_MODEL_EFFORT_LOW_ID, COMPOSER_MODEL_EFFORT_MEDIUM_ID,
    COMPOSER_MODEL_EFFORT_XHIGH_ID, COMPOSER_MODEL_MODELS_ID, COMPOSER_MODEL_RESET_ID,
    COMPOSER_MODEL_SPEEDS_ID, COMPOSER_MODEL_SPEED_ID, COMPOSER_PERMISSION_AUTO_ID,
    COMPOSER_PERMISSION_CUSTOM_ID, COMPOSER_PERMISSION_FULL_ID, COMPOSER_PERMISSION_REQUEST_ID,
    ROW_H, SECTION_H,
};
use crate::{stable_widget_id, SemanticIcon, WidgetId};

#[derive(Debug, Clone)]
pub(super) enum MenuEntry {
    Section(String),
    Row(ComposerFooterRowLayout, bool),
}

impl MenuEntry {
    pub(super) fn height(&self) -> f32 {
        match self {
            Self::Section(_) => SECTION_H,
            Self::Row(_, true) => COMPACT_ROW_H,
            Self::Row(_, false) => ROW_H,
        }
    }
}

pub(super) fn permission_entries(state: &ZodeAppState) -> Vec<MenuEntry> {
    let locked = runtime_controls_locked(state);
    let detail = |normal| {
        locked
            .then_some("任务运行中，完成后可更改")
            .or(Some(normal))
    };
    let selected = |approval, mode, network| {
        state.composer.approval_mode == approval
            && state.composer.sandbox_mode == mode
            && state.composer.sandbox_network == network
    };
    vec![
        MenuEntry::Section("应如何批准 Zode 操作？".into()),
        entry(
            COMPOSER_PERMISSION_REQUEST_ID,
            "请求批准",
            detail("执行写入或高风险操作前始终询问"),
            SemanticIcon::Host,
            !locked,
            selected(ApprovalMode::Request, SandboxMode::WorkspaceWrite, false),
            false,
        ),
        entry(
            COMPOSER_PERMISSION_AUTO_ID,
            "替我审批",
            detail("仅对检测到的风险操作请求批准"),
            SemanticIcon::Refresh,
            !locked,
            selected(ApprovalMode::Auto, SandboxMode::WorkspaceWrite, true),
            false,
        ),
        entry(
            COMPOSER_PERMISSION_FULL_ID,
            "完全访问权限",
            detail("可不受限制地访问互联网和电脑上的文件"),
            SemanticIcon::ShieldAlert,
            !locked,
            selected(ApprovalMode::Full, SandboxMode::Off, false),
            false,
        ),
        entry(
            COMPOSER_PERMISSION_CUSTOM_ID,
            "自定义配置",
            detail("使用 ~/.zode/config.json 中定义的权限"),
            SemanticIcon::Settings,
            !locked,
            false,
            false,
        ),
    ]
}

pub(super) fn add_entries(state: &ZodeAppState) -> Vec<MenuEntry> {
    let mut entries = vec![
        MenuEntry::Section("添加".into()),
        entry(
            COMPOSER_ADD_FILE_ID,
            "文件和文件夹",
            Some("当前仅支持从剪贴板粘贴图片"),
            SemanticIcon::FileText,
            false,
            false,
            true,
        ),
        entry(
            COMPOSER_ADD_WECHAT_ID,
            "附加微信",
            Some("当前版本尚未接入"),
            SemanticIcon::Chat,
            false,
            false,
            true,
        ),
        entry(
            COMPOSER_ADD_GOAL_ID,
            "目标",
            Some("当前版本尚未接入"),
            SemanticIcon::ExploreCode,
            false,
            false,
            true,
        ),
        entry(
            COMPOSER_ADD_PLAN_ID,
            "计划模式",
            Some("当前版本尚未接入"),
            SemanticIcon::Configuration,
            false,
            false,
            true,
        ),
    ];
    let subagents = state
        .current_session
        .as_ref()
        .and_then(|session| state.presentation.sessions.get(session))
        .and_then(|presentation| presentation.context.ready())
        .map(|context| context.subagents.as_slice())
        .unwrap_or_default();
    if !subagents.is_empty() {
        entries.push(MenuEntry::Section("智能体".into()));
        entries.extend(subagents.iter().map(|agent| {
            entry(
                stable_widget_id(0x7e, &agent.id),
                agent.label.clone(),
                agent.value.as_deref(),
                SemanticIcon::Sparkles,
                false,
                false,
                true,
            )
        }));
    }
    entries
}

pub(super) fn model_entries(state: &ZodeAppState) -> Vec<MenuEntry> {
    match state.composer.footer_menu {
        Some(ComposerFooterMenu::ModelModels) => model_choice_entries(state),
        Some(ComposerFooterMenu::ModelEffort) => effort_entries(state),
        Some(ComposerFooterMenu::ModelSpeed) => speed_entries(),
        _ => model_root_entries(state),
    }
}

fn model_root_entries(state: &ZodeAppState) -> Vec<MenuEntry> {
    let locked = runtime_controls_locked(state);
    let locked_detail = locked.then_some("任务运行中，完成后可更改");
    vec![
        MenuEntry::Section("设置".into()),
        entry(
            COMPOSER_MODEL_MODELS_ID,
            "模型",
            Some(if state.provider_setup_required {
                "Codex 尚未配置"
            } else {
                state.composer.model.as_deref().unwrap_or("选择模型")
            }),
            SemanticIcon::Sparkles,
            true,
            false,
            false,
        ),
        entry(
            COMPOSER_MODEL_EFFORTS_ID,
            "推理强度",
            Some(
                state
                    .composer
                    .effort
                    .as_deref()
                    .map(effort_label)
                    .unwrap_or("默认"),
            ),
            SemanticIcon::Configuration,
            true,
            false,
            false,
        ),
        entry(
            COMPOSER_MODEL_SPEEDS_ID,
            "速度",
            Some("标准"),
            SemanticIcon::Refresh,
            true,
            false,
            false,
        ),
        entry(
            COMPOSER_MODEL_RESET_ID,
            "重置为默认设置",
            locked_detail,
            SemanticIcon::Refresh,
            state.composer_defaults.is_some() && !locked,
            false,
            true,
        ),
    ]
}

fn model_choice_entries(state: &ZodeAppState) -> Vec<MenuEntry> {
    let locked = runtime_controls_locked(state);
    let locked_detail = locked.then_some("任务运行中，完成后可更改");
    let mut entries = vec![back_entry(), MenuEntry::Section("模型".into())];
    if state.provider_setup_required {
        entries.push(entry(
            COMPOSER_MODEL_CONFIGURE_ID,
            "Codex（内置）· 尚未配置",
            Some("前往配置页查看连接说明"),
            SemanticIcon::Configuration,
            true,
            false,
            true,
        ));
    }
    entries.extend(state.composer.available_models.iter().map(|model| {
        entry(
            stable_widget_id(0x7d, model),
            model,
            locked_detail,
            SemanticIcon::Sparkles,
            !locked,
            state.composer.model.as_deref() == Some(model.as_str()),
            true,
        )
    }));
    if state.composer.available_models.is_empty() && !state.provider_setup_required {
        entries.push(entry(
            COMPOSER_MODEL_CONFIGURE_ID,
            "暂无可用模型",
            Some("运行时未返回模型列表"),
            SemanticIcon::Configuration,
            false,
            false,
            true,
        ));
    }
    entries
}

fn effort_entries(state: &ZodeAppState) -> Vec<MenuEntry> {
    let locked = runtime_controls_locked(state);
    let locked_detail = locked.then_some("任务运行中，完成后可更改");
    let mut entries = vec![back_entry(), MenuEntry::Section("推理强度".into())];
    for (id, value) in [
        (COMPOSER_MODEL_EFFORT_LOW_ID, "low"),
        (COMPOSER_MODEL_EFFORT_MEDIUM_ID, "medium"),
        (COMPOSER_MODEL_EFFORT_HIGH_ID, "high"),
        (COMPOSER_MODEL_EFFORT_XHIGH_ID, "xhigh"),
    ] {
        entries.push(entry(
            id,
            effort_label(value),
            locked_detail,
            SemanticIcon::Configuration,
            !locked,
            state.composer.effort.as_deref() == Some(value),
            true,
        ));
    }
    entries
}

fn speed_entries() -> Vec<MenuEntry> {
    vec![
        back_entry(),
        MenuEntry::Section("速度".into()),
        entry(
            COMPOSER_MODEL_SPEED_ID,
            "标准",
            Some("当前仅支持标准速度"),
            SemanticIcon::Refresh,
            false,
            true,
            true,
        ),
    ]
}

fn back_entry() -> MenuEntry {
    entry(
        COMPOSER_MODEL_BACK_ID,
        "返回模型设置",
        None,
        SemanticIcon::Back,
        true,
        false,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn entry(
    id: WidgetId,
    label: impl Into<String>,
    detail: Option<&str>,
    icon: SemanticIcon,
    enabled: bool,
    selected: bool,
    compact: bool,
) -> MenuEntry {
    MenuEntry::Row(
        ComposerFooterRowLayout {
            id,
            rect: Rect::xywh(0.0, 0.0, 0.0, 0.0),
            label: label.into(),
            detail: detail.map(str::to_owned),
            icon,
            enabled,
            selected,
        },
        compact,
    )
}
