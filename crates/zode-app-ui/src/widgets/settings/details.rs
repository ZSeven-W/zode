use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{
    IntegrationCategory, LoadState, LocalSettingFact, SettingsCategory, ZodeAppState,
};

use super::row::{paint_card, paint_divider, paint_heading, paint_section_label, SECTION_TOP};
use crate::{paint_single_line, RectExt, ZodeTheme};

const SECTION_LABEL_HEIGHT: f32 = 24.0;
const SECTION_LABEL_GAP: f32 = 10.0;
const SECTION_GAP: f32 = 34.0;
const ROW_HEIGHT: f32 = 58.0;
const EMPTY_HEIGHT: f32 = 92.0;
const BOTTOM_GAP: f32 = 24.0;

#[derive(Debug, Clone, PartialEq, Eq)]
struct DetailSection {
    title: String,
    rows: Vec<LocalSettingFact>,
    empty: Option<String>,
}

impl DetailSection {
    fn facts(title: impl Into<String>, rows: Vec<LocalSettingFact>) -> Self {
        Self {
            title: title.into(),
            rows,
            empty: None,
        }
    }

    fn empty(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            rows: Vec::new(),
            empty: Some(message.into()),
        }
    }

    fn height(&self) -> f32 {
        SECTION_LABEL_HEIGHT
            + SECTION_LABEL_GAP
            + if self.rows.is_empty() {
                EMPTY_HEIGHT
            } else {
                ROW_HEIGHT * self.rows.len() as f32
            }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DetailPage {
    title: &'static str,
    subtitle: &'static str,
    sections: Vec<DetailSection>,
}

pub(super) fn content_height(state: &ZodeAppState, category: SettingsCategory) -> f32 {
    let page = page(state, category);
    SECTION_TOP
        + page.sections.iter().map(DetailSection::height).sum::<f32>()
        + SECTION_GAP * page.sections.len().saturating_sub(1) as f32
        + BOTTOM_GAP
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    content: Rect,
    state: &ZodeAppState,
    category: SettingsCategory,
    offset: f32,
    theme: &ZodeTheme,
) {
    let page = page(state, category);
    paint_heading(painter, content, page.title, page.subtitle, offset, theme);
    let mut y = content.origin.y + SECTION_TOP - offset;
    for section in page.sections {
        let label = Rect::xywh(content.origin.x, y, content.size.x, SECTION_LABEL_HEIGHT);
        paint_section_label(painter, label, &section.title, theme);
        y = label.max_y() + SECTION_LABEL_GAP;
        let card_height = if section.rows.is_empty() {
            EMPTY_HEIGHT
        } else {
            ROW_HEIGHT * section.rows.len() as f32
        };
        let card = Rect::xywh(content.origin.x, y, content.size.x, card_height);
        paint_card(painter, card, theme);
        if let Some(message) = section.empty {
            paint_single_line(
                painter,
                &message,
                Rect::xywh(
                    card.origin.x + 18.0,
                    card.origin.y,
                    (card.size.x - 36.0).max(0.0),
                    card.size.y,
                ),
                12.0,
                450,
                theme.tokens.muted_foreground,
                HorizontalAlign::Start,
            );
        } else {
            for (index, fact) in section.rows.iter().enumerate() {
                if index > 0 {
                    paint_divider(painter, card, index as f32 * ROW_HEIGHT, theme);
                }
                let row = Rect::xywh(
                    card.origin.x,
                    card.origin.y + index as f32 * ROW_HEIGHT,
                    card.size.x,
                    ROW_HEIGHT,
                );
                let value_width = (row.size.x * 0.56).max(0.0);
                paint_single_line(
                    painter,
                    &fact.label,
                    Rect::xywh(
                        row.origin.x + 18.0,
                        row.origin.y,
                        (row.size.x - value_width - 48.0).max(0.0),
                        row.size.y,
                    ),
                    13.0,
                    550,
                    theme.tokens.foreground,
                    HorizontalAlign::Start,
                );
                paint_single_line(
                    painter,
                    &fact.value,
                    Rect::xywh(
                        row.max_x() - value_width - 18.0,
                        row.origin.y,
                        value_width,
                        row.size.y,
                    ),
                    12.0,
                    400,
                    theme.tokens.muted_foreground,
                    HorizontalAlign::End,
                );
            }
        }
        y = card.max_y() + SECTION_GAP;
    }
}

fn page(state: &ZodeAppState, category: SettingsCategory) -> DetailPage {
    match category {
        SettingsCategory::Profile => DetailPage {
            title: "个人资料",
            subtitle: "本机资料",
            sections: vec![
                DetailSection::facts(
                    "本地账户",
                    vec![fact("用户", state.local_profile.display_name.clone())],
                ),
                DetailSection::empty(
                    "活动统计",
                    "当前桌面端没有账户，无法提供 Token 数或任务天数。",
                ),
            ],
        },
        SettingsCategory::Voice => {
            unavailable("语音", "语音输入", "当前节点没有提供可配置的语音输入设备。")
        }
        SettingsCategory::Configuration => loaded_page(
            "配置",
            "本机有效配置",
            "安全配置摘要",
            &state.local_settings.configuration,
            "尚未读取本机配置。",
        ),
        SettingsCategory::Personalization => unavailable(
            "个性化",
            "自定义指令与记忆",
            "当前桌面协议尚未提供可编辑的自定义指令与记忆设置。",
        ),
        SettingsCategory::Pets => {
            unavailable("宠物", "桌面宠物", "当前安装未发现可配置的桌面宠物。")
        }
        SettingsCategory::KeyboardShortcuts => DetailPage {
            title: "键盘快捷键",
            subtitle: "当前桌面端可用的快捷键",
            sections: vec![DetailSection::facts(
                "应用",
                vec![
                    fact("打开终端", primary_shortcut("`")),
                    fact("切换侧栏任务", primary_shortcut("1…5")),
                    fact("切换位置", "Tab / Shift+Tab"),
                    fact("返回应用", "Esc"),
                    fact("复制终端选区", primary_shortcut("C")),
                ],
            )],
        },
        SettingsCategory::Usage => usage_page(state),
        SettingsCategory::Account => DetailPage {
            title: "账户",
            subtitle: "本机用户",
            sections: vec![DetailSection::facts(
                "账户状态",
                vec![
                    fact("本机用户", state.local_profile.display_name.clone()),
                    fact("账户", "未连接"),
                ],
            )],
        },
        SettingsCategory::AppSnapshots => {
            unavailable("应用快照", "已连接应用", "当前节点未提供应用快照。")
        }
        SettingsCategory::Browser => loaded_page(
            "浏览器",
            "内置 Managed Browser",
            "有效配置",
            &state.local_settings.browser,
            "尚未读取浏览器配置。",
        ),
        SettingsCategory::ComputerUse => unavailable(
            "电脑操控",
            "本机能力",
            "当前节点未提供独立的电脑操控设置数据。",
        ),
        SettingsCategory::Hooks => loaded_page(
            "钩子",
            "全局与项目 hooks.json",
            "已配置钩子",
            &state.local_settings.hooks,
            "尚未读取钩子配置。",
        ),
        SettingsCategory::Connectors => connectors_page(state),
        SettingsCategory::Git => loaded_page(
            "Git",
            "当前项目仓库",
            "仓库信息",
            &state.local_settings.git,
            "当前项目没有可读取的 Git 仓库。",
        ),
        SettingsCategory::Environment => environment_page(state),
        SettingsCategory::Worktree => loaded_page(
            "工作树",
            "当前 Git 仓库的工作树",
            "已发现工作树",
            &state.local_settings.worktrees,
            "当前项目没有可读取的 Git 工作树。",
        ),
        SettingsCategory::General
        | SettingsCategory::Appearance
        | SettingsCategory::Permissions
        | SettingsCategory::ArchivedTasks => unreachable!("handled by dedicated settings pages"),
    }
}

fn loaded_page(
    title: &'static str,
    subtitle: &'static str,
    section: &'static str,
    load: &LoadState<Vec<LocalSettingFact>>,
    idle: &'static str,
) -> DetailPage {
    let detail = match load {
        LoadState::Ready(rows) if !rows.is_empty() => DetailSection::facts(section, rows.clone()),
        LoadState::Ready(_) => DetailSection::empty(section, "未发现已配置项目。"),
        LoadState::Idle => DetailSection::empty(section, idle),
        LoadState::Loading => DetailSection::empty(section, "正在读取本机状态…"),
        LoadState::Failed(message) => DetailSection::empty(section, format!("读取失败：{message}")),
    };
    DetailPage {
        title,
        subtitle,
        sections: vec![detail],
    }
}

fn usage_page(state: &ZodeAppState) -> DetailPage {
    let section = state
        .current_session
        .as_ref()
        .and_then(|session| state.usage.get(session))
        .map(|usage| {
            let mut rows = vec![
                fact("输入 Token", usage.input_tokens.to_string()),
                fact("输出 Token", usage.output_tokens.to_string()),
            ];
            if let Some(context) = usage.context_used {
                rows.push(fact("上下文用量", format!("{:.1}%", context * 100.0)));
            }
            if let Some(cost) = usage.cost_usd {
                rows.push(fact("当前任务成本", format!("US${cost:.4}")));
            }
            DetailSection::facts("当前任务", rows)
        })
        .unwrap_or_else(|| DetailSection::empty("当前任务", "选择任务后可查看本次会话用量。"));
    DetailPage {
        title: "使用情况和计费",
        subtitle: "仅显示本次任务用量",
        sections: vec![
            section,
            DetailSection::empty("计划与点数", "当前节点未提供计划、点数或重置日期。"),
        ],
    }
}

fn connectors_page(state: &ZodeAppState) -> DetailPage {
    let section = match &state.presentation.integrations {
        LoadState::Ready(catalog) => {
            let rows = catalog
                .all_entries()
                .filter(|entry| entry.category == IntegrationCategory::Mcp)
                .map(|entry| fact(entry.name.clone(), entry.availability.label()))
                .collect::<Vec<_>>();
            if rows.is_empty() {
                DetailSection::empty("MCP 连接器", "当前工作区未提供 MCP 连接器。")
            } else {
                DetailSection::facts("MCP 连接器", rows)
            }
        }
        LoadState::Loading => DetailSection::empty("MCP 连接器", "正在读取本机集成…"),
        LoadState::Failed(message) => {
            DetailSection::empty("MCP 连接器", format!("读取失败：{message}"))
        }
        LoadState::Idle => DetailSection::empty("MCP 连接器", "打开插件页后读取本机连接器。"),
    };
    DetailPage {
        title: "连接器",
        subtitle: "当前工作区的 MCP 状态",
        sections: vec![section],
    }
}

fn environment_page(state: &ZodeAppState) -> DetailPage {
    let rows = state
        .projects
        .iter()
        .map(|project| {
            fact(
                workspace_label(project.workspace_uri.as_str()),
                format!(
                    "{} · {}",
                    if project.available {
                        "可用"
                    } else {
                        "不可用"
                    },
                    project.workspace_uri.as_str()
                ),
            )
        })
        .collect::<Vec<_>>();
    let section = if rows.is_empty() {
        DetailSection::empty("项目环境", "当前桌面端没有已知项目。")
    } else {
        DetailSection::facts("项目环境", rows)
    };
    DetailPage {
        title: "环境",
        subtitle: "本机已知项目",
        sections: vec![section],
    }
}

fn unavailable(title: &'static str, subtitle: &'static str, message: &'static str) -> DetailPage {
    DetailPage {
        title,
        subtitle,
        sections: vec![DetailSection::empty("当前状态", message)],
    }
}

fn fact(label: impl Into<String>, value: impl Into<String>) -> LocalSettingFact {
    LocalSettingFact {
        label: label.into(),
        value: value.into(),
    }
}

fn primary_shortcut(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("⌘{key}")
    } else {
        format!("Ctrl+{key}")
    }
}

fn workspace_label(uri: &str) -> String {
    uri.trim_end_matches('/')
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or(uri)
        .to_owned()
}
