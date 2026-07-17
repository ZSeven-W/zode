use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{AppCommand, IntegrationsTab, SettingsCategory, ShellRoute, ZodeAppState};

use crate::{paint_single_line, RectExt, SemanticIcon, WidgetId, ZodeTheme};

pub(super) const SETTINGS_GENERAL_CATEGORY_ID: WidgetId = WidgetId(80);
pub(super) const SETTINGS_APPEARANCE_CATEGORY_ID: WidgetId = WidgetId(81);
pub(super) const SETTINGS_PERMISSIONS_CATEGORY_ID: WidgetId = WidgetId(82);
pub(super) const SETTINGS_KEYBOARD_SHORTCUTS_CATEGORY_ID: WidgetId = WidgetId(83);
pub(super) const SETTINGS_ENVIRONMENT_CATEGORY_ID: WidgetId = WidgetId(84);
pub const SETTINGS_BACK_ID: WidgetId = WidgetId(85);

const SETTINGS_PROFILE_ID: WidgetId = WidgetId(8_101);
const SETTINGS_VOICE_ID: WidgetId = WidgetId(8_102);
const SETTINGS_CONFIGURATION_ID: WidgetId = WidgetId(8_103);
const SETTINGS_PERSONALIZATION_ID: WidgetId = WidgetId(8_104);
const SETTINGS_PETS_ID: WidgetId = WidgetId(8_105);
const SETTINGS_USAGE_ID: WidgetId = WidgetId(8_106);
const SETTINGS_ACCOUNT_ID: WidgetId = WidgetId(8_107);
const SETTINGS_APP_SNAPSHOTS_ID: WidgetId = WidgetId(8_108);
const SETTINGS_PLUGINS_ID: WidgetId = WidgetId(8_109);
const SETTINGS_BROWSER_ID: WidgetId = WidgetId(8_110);
const SETTINGS_COMPUTER_USE_ID: WidgetId = WidgetId(8_111);
const SETTINGS_HOOKS_ID: WidgetId = WidgetId(8_112);
const SETTINGS_CONNECTORS_ID: WidgetId = WidgetId(8_113);
const SETTINGS_GIT_ID: WidgetId = WidgetId(8_114);
const SETTINGS_WORKTREE_ID: WidgetId = WidgetId(8_115);
const SETTINGS_ARCHIVED_ID: WidgetId = WidgetId(8_116);

const ROW_HEIGHT: f32 = 28.0;
const GROUP_LABEL_HEIGHT: f32 = 20.0;
const GROUP_GAP: f32 = 8.0;

#[derive(Debug, Clone, Copy)]
struct NavigationDescriptor {
    id: WidgetId,
    group: &'static str,
    label: &'static str,
    icon: SemanticIcon,
    target: Option<NavigationTarget>,
}

#[derive(Debug, Clone, Copy)]
enum NavigationTarget {
    Settings(SettingsCategory),
    Plugins,
}

impl NavigationTarget {
    fn command(self) -> AppCommand {
        match self {
            Self::Settings(category) => AppCommand::SelectSettingsCategory(category),
            Self::Plugins => {
                AppCommand::Navigate(ShellRoute::Integrations(IntegrationsTab::Plugins))
            }
        }
    }

    fn selected(self, route: ShellRoute) -> bool {
        match (self, route) {
            (Self::Settings(expected), ShellRoute::Settings(actual)) => expected == actual,
            (Self::Plugins, ShellRoute::Integrations(IntegrationsTab::Plugins)) => true,
            _ => false,
        }
    }
}

const NAVIGATION: [NavigationDescriptor; 20] = [
    descriptor(
        SETTINGS_GENERAL_CATEGORY_ID,
        "个人",
        "常规",
        SemanticIcon::Settings,
        Some(NavigationTarget::Settings(SettingsCategory::General)),
    ),
    descriptor(
        SETTINGS_PROFILE_ID,
        "个人",
        "个人资料",
        SemanticIcon::User,
        None,
    ),
    descriptor(
        SETTINGS_APPEARANCE_CATEGORY_ID,
        "个人",
        "外观",
        SemanticIcon::Appearance,
        Some(NavigationTarget::Settings(SettingsCategory::Appearance)),
    ),
    descriptor(
        SETTINGS_VOICE_ID,
        "个人",
        "语音",
        SemanticIcon::Microphone,
        None,
    ),
    descriptor(
        SETTINGS_CONFIGURATION_ID,
        "个人",
        "配置",
        SemanticIcon::Configuration,
        None,
    ),
    descriptor(
        SETTINGS_PERSONALIZATION_ID,
        "个人",
        "个性化",
        SemanticIcon::Sparkles,
        None,
    ),
    descriptor(SETTINGS_PETS_ID, "个人", "宠物", SemanticIcon::Pet, None),
    descriptor(
        SETTINGS_KEYBOARD_SHORTCUTS_CATEGORY_ID,
        "个人",
        "键盘快捷键",
        SemanticIcon::Keyboard,
        None,
    ),
    descriptor(
        SETTINGS_USAGE_ID,
        "个人",
        "使用情况和计费",
        SemanticIcon::Usage,
        None,
    ),
    descriptor(
        SETTINGS_ACCOUNT_ID,
        "个人",
        "账户",
        SemanticIcon::Account,
        None,
    ),
    descriptor(
        SETTINGS_APP_SNAPSHOTS_ID,
        "集成",
        "应用快照",
        SemanticIcon::Snapshot,
        None,
    ),
    descriptor(
        SETTINGS_PLUGINS_ID,
        "集成",
        "插件",
        SemanticIcon::Integrations,
        Some(NavigationTarget::Plugins),
    ),
    descriptor(
        SETTINGS_BROWSER_ID,
        "集成",
        "浏览器",
        SemanticIcon::Browser,
        None,
    ),
    descriptor(
        SETTINGS_COMPUTER_USE_ID,
        "集成",
        "电脑操控",
        SemanticIcon::Computer,
        None,
    ),
    descriptor(SETTINGS_HOOKS_ID, "编码", "钩子", SemanticIcon::Hook, None),
    descriptor(
        SETTINGS_CONNECTORS_ID,
        "编码",
        "连接器",
        SemanticIcon::Connect,
        None,
    ),
    descriptor(SETTINGS_GIT_ID, "编码", "Git", SemanticIcon::Git, None),
    descriptor(
        SETTINGS_ENVIRONMENT_CATEGORY_ID,
        "编码",
        "环境",
        SemanticIcon::Environment,
        None,
    ),
    descriptor(
        SETTINGS_WORKTREE_ID,
        "编码",
        "工作树",
        SemanticIcon::Worktree,
        None,
    ),
    descriptor(
        SETTINGS_ARCHIVED_ID,
        "已归档",
        "已归档任务",
        SemanticIcon::Archive,
        Some(NavigationTarget::Settings(SettingsCategory::ArchivedTasks)),
    ),
];

const fn descriptor(
    id: WidgetId,
    group: &'static str,
    label: &'static str,
    icon: SemanticIcon,
    target: Option<NavigationTarget>,
) -> NavigationDescriptor {
    NavigationDescriptor {
        id,
        group,
        label,
        icon,
        target,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsNavigationGroupLayout {
    pub label: &'static str,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsNavigationEntryLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub icon_rect: Rect,
    pub label_rect: Rect,
    pub status_rect: Rect,
    pub group: &'static str,
    pub label: &'static str,
    pub icon: SemanticIcon,
    pub selected: bool,
    pub enabled: bool,
    pub command: Option<AppCommand>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsNavigationLayout {
    pub title: Rect,
    pub search: Rect,
    pub groups: Vec<SettingsNavigationGroupLayout>,
    pub entries: Vec<SettingsNavigationEntryLayout>,
}

pub(super) fn layout(rect: Rect, state: &ZodeAppState) -> SettingsNavigationLayout {
    let title = Rect::xywh(
        rect.origin.x + 8.0,
        rect.origin.y + 48.0,
        (rect.size.x - 16.0).max(0.0),
        30.0,
    );
    let search = Rect::xywh(
        rect.origin.x + 8.0,
        rect.origin.y + 86.0,
        (rect.size.x - 16.0).max(0.0),
        30.0,
    );
    let mut y = rect.origin.y + 130.0;
    let mut previous_group = "";
    let mut groups = Vec::new();
    let mut entries = Vec::with_capacity(NAVIGATION.len());
    for descriptor in NAVIGATION {
        if descriptor.group != previous_group {
            if !previous_group.is_empty() {
                y += GROUP_GAP;
            }
            groups.push(SettingsNavigationGroupLayout {
                label: descriptor.group,
                rect: Rect::xywh(
                    rect.origin.x + 16.0,
                    y,
                    (rect.size.x - 32.0).max(0.0),
                    GROUP_LABEL_HEIGHT,
                ),
            });
            y += GROUP_LABEL_HEIGHT;
            previous_group = descriptor.group;
        }
        let row = Rect::xywh(
            rect.origin.x + 8.0,
            y,
            (rect.size.x - 16.0).max(0.0),
            ROW_HEIGHT,
        );
        let icon_rect = Rect::xywh(
            row.origin.x + 10.0,
            row.origin.y + (ROW_HEIGHT - 16.0) / 2.0,
            16.0,
            16.0,
        );
        let status_width = 58.0_f32.min((row.size.x - 42.0).max(0.0));
        let status_rect = Rect::xywh(
            row.max_x() - status_width - 8.0,
            row.origin.y,
            status_width,
            row.size.y,
        );
        let label_rect = Rect::xywh(
            icon_rect.max_x() + 8.0,
            row.origin.y,
            (status_rect.origin.x - icon_rect.max_x() - 12.0).max(0.0),
            row.size.y,
        );
        let command = descriptor.target.map(NavigationTarget::command);
        entries.push(SettingsNavigationEntryLayout {
            id: descriptor.id,
            rect: row,
            icon_rect,
            label_rect,
            status_rect,
            group: descriptor.group,
            label: descriptor.label,
            icon: descriptor.icon,
            selected: descriptor
                .target
                .is_some_and(|target| target.selected(state.presentation.route)),
            enabled: command.is_some(),
            command,
        });
        y += ROW_HEIGHT;
    }
    SettingsNavigationLayout {
        title,
        search,
        groups,
        entries,
    }
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    rect: Rect,
    layout: &SettingsNavigationLayout,
    theme: &ZodeTheme,
) {
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
        return;
    }
    painter.fill_rect(rect, theme.sidebar);
    painter.save();
    painter.clip_rect(rect);
    let back_icon_size = 16.0_f32.min(layout.title.size.y);
    painter.stroke_svg_path(
        SemanticIcon::Back.path(),
        jian_widgets::Point2D::new(
            layout.title.origin.x + 8.0,
            layout.title.origin.y + (layout.title.size.y - back_icon_size) / 2.0,
        ),
        back_icon_size,
        theme.tokens.muted_foreground,
        SemanticIcon::Back.stroke_width(),
    );
    paint_single_line(
        painter,
        "返回应用",
        Rect::xywh(
            layout.title.origin.x + 32.0,
            layout.title.origin.y,
            (layout.title.size.x - 40.0).max(0.0),
            layout.title.size.y,
        ),
        13.0,
        450,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
    painter.fill_round_rect(layout.search, 8.0, theme.tokens.card);
    painter.stroke_round_rect(layout.search, 8.0, theme.tokens.border, 1.0);
    let search_icon = Rect::xywh(
        layout.search.origin.x + 10.0,
        layout.search.origin.y + 7.0,
        16.0,
        16.0,
    );
    painter.stroke_svg_path(
        SemanticIcon::Search.path(),
        search_icon.origin,
        search_icon.size.x,
        theme.tokens.muted_foreground,
        SemanticIcon::Search.stroke_width(),
    );
    paint_single_line(
        painter,
        "搜索设置…",
        Rect::xywh(
            search_icon.max_x() + 7.0,
            layout.search.origin.y,
            (layout.search.max_x() - search_icon.max_x() - 17.0).max(0.0),
            layout.search.size.y,
        ),
        12.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
    for group in &layout.groups {
        paint_single_line(
            painter,
            group.label,
            group.rect,
            11.0,
            500,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
    }
    for entry in &layout.entries {
        if entry.selected {
            painter.fill_round_rect(entry.rect, 9.0, theme.tokens.row_selected);
        }
        let color = if entry.enabled || entry.selected {
            theme.sidebar_foreground
        } else {
            theme.tokens.muted_foreground.with_alpha(0.62)
        };
        painter.stroke_svg_path(
            entry.icon.path(),
            entry.icon_rect.origin,
            entry.icon_rect.size.x,
            color,
            entry.icon.stroke_width(),
        );
        paint_single_line(
            painter,
            entry.label,
            entry.label_rect,
            13.0,
            if entry.selected { 600 } else { 450 },
            color,
            HorizontalAlign::Start,
        );
        if !entry.enabled {
            paint_single_line(
                painter,
                "即将支持",
                entry.status_rect,
                9.0,
                450,
                theme.tokens.muted_foreground.with_alpha(0.72),
                HorizontalAlign::End,
            );
        }
    }
    painter.restore();
}

pub(super) fn command_for_widget(id: WidgetId) -> Option<AppCommand> {
    if id == SETTINGS_BACK_ID {
        return Some(AppCommand::Navigate(ShellRoute::Conversation));
    }
    NAVIGATION
        .iter()
        .find(|descriptor| descriptor.id == id)
        .and_then(|descriptor| descriptor.target)
        .map(NavigationTarget::command)
}

pub(super) const fn category_widget_id(category: SettingsCategory) -> WidgetId {
    match category {
        SettingsCategory::General => SETTINGS_GENERAL_CATEGORY_ID,
        SettingsCategory::Appearance => SETTINGS_APPEARANCE_CATEGORY_ID,
        SettingsCategory::Permissions => SETTINGS_PERMISSIONS_CATEGORY_ID,
        SettingsCategory::KeyboardShortcuts => SETTINGS_KEYBOARD_SHORTCUTS_CATEGORY_ID,
        SettingsCategory::Environment => SETTINGS_ENVIRONMENT_CATEGORY_ID,
        SettingsCategory::ArchivedTasks => SETTINGS_ARCHIVED_ID,
    }
}
