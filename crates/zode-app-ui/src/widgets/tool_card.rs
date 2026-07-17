use jian_widgets::{HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::default_tool_expanded;
use zode_node_protocol::{ToolCall, ToolStatus};

use crate::{paint_single_line, SemanticIcon, ZodeTheme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTone {
    Running,
    Success,
    Failure,
}

pub struct ToolCard;

impl ToolCard {
    pub fn default_expanded(tool: &ToolCall) -> bool {
        default_tool_expanded(&tool.name)
    }

    /// Whether this tool shows the always-visible computer-use permission
    /// hint (destructive color, reserved even when collapsed). Scoped to the
    /// computer tools so an ordinary tool's `detail` (branch names, test
    /// results) keeps its muted, expand-only rendering. Single source of
    /// truth for both painting and height estimation.
    pub fn shows_permission_hint(tool: &ToolCall) -> bool {
        let is_computer_tool =
            tool.name == COMPUTER_READ_TOOL_NAME || tool.name == COMPUTER_ACT_TOOL_NAME;
        is_computer_tool
            && tool
                .detail
                .as_deref()
                .is_some_and(|detail| !detail.is_empty())
    }

    pub const fn tone(tool: &ToolCall) -> ToolTone {
        match tool.status {
            ToolStatus::Running => ToolTone::Running,
            ToolStatus::Completed => ToolTone::Success,
            ToolStatus::Failed => ToolTone::Failure,
        }
    }

    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        tool: &ToolCall,
        expanded: bool,
        theme: &ZodeTheme,
    ) {
        let tone = Self::tone(tool);
        let icon_color = match tone {
            ToolTone::Running | ToolTone::Success => theme.tokens.muted_foreground,
            ToolTone::Failure => theme.tokens.destructive,
        };
        let header_height = rect.size.y.min(35.0);
        let header = Rect::xywh(rect.origin.x, rect.origin.y, rect.size.x, header_height);
        let (label, icon) = action_presentation(tool, tone);
        painter.stroke_svg_path(
            icon.path(),
            Point2D::new(
                rect.origin.x,
                header.origin.y + (header.size.y - 15.0) / 2.0,
            ),
            15.0,
            icon_color,
            icon.stroke_width(),
        );
        paint_single_line(
            painter,
            &label,
            Rect::xywh(
                rect.origin.x + 23.0,
                header.origin.y,
                (rect.size.x - 23.0).max(0.0),
                header.size.y,
            ),
            14.0,
            500,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
        // The permission-pending hint (computer-use TCC retry - see
        // `engine_backend::permission_pending_detail`) must stay visible even
        // when the card is collapsed, and is rendered in destructive color.
        // It is scoped to the computer tools: `detail` is otherwise the
        // ordinary expanded-summary fallback (branch names, test results,
        // etc.) which must keep its muted, expand-only rendering below.
        if let Some(pending) = tool
            .detail
            .as_deref()
            .filter(|_| Self::shows_permission_hint(tool))
        {
            paint_single_line(
                painter,
                pending,
                Rect::xywh(
                    rect.origin.x + 23.0,
                    rect.origin.y + header_height,
                    (rect.size.x - 23.0).max(0.0),
                    (rect.size.y - header_height).max(0.0),
                ),
                13.0,
                500,
                theme.tokens.destructive,
                HorizontalAlign::Start,
            );
        } else if expanded {
            let detail = if tool.summary.trim().is_empty() {
                tool.detail.as_deref().unwrap_or("")
            } else {
                &tool.summary
            };
            paint_single_line(
                painter,
                detail,
                Rect::xywh(
                    rect.origin.x + 23.0,
                    rect.origin.y + header_height,
                    (rect.size.x - 23.0).max(0.0),
                    (rect.size.y - header_height).max(0.0),
                ),
                13.0,
                400,
                theme.tokens.muted_foreground.with_alpha(0.72),
                HorizontalAlign::Start,
            );
        }
    }
}

/// The `agent-tools-code` `Task` tool's stable name. Matched exactly (not by
/// fuzzy substring like every other category below) because the sub-agent
/// row derives its label from the tool's own real `agent_type`/`description`,
/// and a name-based guess would be wrong to apply to an unrelated tool whose
/// name happens to contain "task" or "agent".
const TASK_TOOL_NAME: &str = "Task";

/// The `zode-core` `computer` tool group's stable names - see
/// `zode_core::computer::tools`. Matched exactly (not by the fuzzy
/// substring scan below) so the card can show a real per-action label
/// (`点击目标` / `输入文本` / …) instead of the generic "使用工具" fallback.
const COMPUTER_READ_TOOL_NAME: &str = "computer_read";
const COMPUTER_ACT_TOOL_NAME: &str = "computer_act";

fn action_presentation(tool: &ToolCall, tone: ToolTone) -> (String, SemanticIcon) {
    if tool.name == TASK_TOOL_NAME {
        return (task_label(tool, tone), SemanticIcon::Sparkles);
    }
    if tool.name == COMPUTER_READ_TOOL_NAME || tool.name == COMPUTER_ACT_TOOL_NAME {
        return (computer_label(tool, tone), SemanticIcon::Computer);
    }
    let name = tool.name.to_ascii_lowercase();
    let (label, icon) = if [
        "read", "list", "search", "find", "get", "export", "snapshot",
    ]
    .iter()
    .any(|needle| name.contains(needle))
    {
        (
            match tone {
                ToolTone::Running => "正在读取",
                ToolTone::Success => "已读取",
                ToolTone::Failure => "读取失败",
            },
            SemanticIcon::FileText,
        )
    } else if [
        "shell", "exec", "command", "terminal", "bash", "zsh", "cargo",
    ]
    .iter()
    .any(|needle| name.contains(needle))
    {
        (
            match tone {
                ToolTone::Running => "正在运行命令",
                ToolTone::Success => "运行了命令",
                ToolTone::Failure => "命令运行失败",
            },
            SemanticIcon::Terminal,
        )
    } else if ["edit", "write", "patch", "update", "insert", "move", "copy"]
        .iter()
        .any(|needle| name.contains(needle))
    {
        (
            match tone {
                ToolTone::Running => "正在编辑",
                ToolTone::Success => "已编辑",
                ToolTone::Failure => "编辑失败",
            },
            SemanticIcon::Edit,
        )
    } else if name.contains("create") {
        (
            match tone {
                ToolTone::Running => "正在创建",
                ToolTone::Success => "已创建",
                ToolTone::Failure => "创建失败",
            },
            SemanticIcon::Plus,
        )
    } else if name.contains("delete") || name.contains("remove") {
        (
            match tone {
                ToolTone::Running => "正在删除",
                ToolTone::Success => "已删除",
                ToolTone::Failure => "删除失败",
            },
            SemanticIcon::Delete,
        )
    } else if name.contains("browser") || name.contains("web") {
        (
            match tone {
                ToolTone::Running => "正在浏览网页",
                ToolTone::Success => "已浏览网页",
                ToolTone::Failure => "浏览失败",
            },
            SemanticIcon::Browser,
        )
    } else {
        (
            match tone {
                ToolTone::Running => "正在使用工具",
                ToolTone::Success => "已使用工具",
                ToolTone::Failure => "工具运行失败",
            },
            SemanticIcon::Hook,
        )
    };
    (label.to_owned(), icon)
}

/// Builds the `Task` card's header label from the tool's own real summary
/// (`"<agent_type>"` or `"<agent_type>: <description>"`, set by the
/// `EventNormalizer`), instead of a generic "子任务" placeholder. Falls back
/// to the placeholder only if the summary is empty or still just the bare
/// tool name (malformed input the normalizer couldn't parse).
fn task_label(tool: &ToolCall, tone: ToolTone) -> String {
    let agent_type = tool
        .summary
        .split(':')
        .next()
        .map(str::trim)
        .filter(|part| !part.is_empty() && *part != TASK_TOOL_NAME)
        .unwrap_or("子任务");
    match tone {
        ToolTone::Running => format!("正在运行 {agent_type}"),
        ToolTone::Success => format!("已运行 {agent_type}"),
        ToolTone::Failure => format!("{agent_type} 失败"),
    }
}

/// Builds the `computer_read`/`computer_act` card's header label from the
/// tool's own structured summary (`"<action> [app=…|element=…|at=…|text=…
/// |key=…]"`, set by `engine_backend::computer_tool_summary`), so the card
/// reads as a real per-action verb ("点击目标" / "输入文本" / …) instead of
/// the generic "使用工具" fallback the substring scan above would otherwise
/// produce for these two tool names.
fn computer_label(tool: &ToolCall, tone: ToolTone) -> String {
    let action = tool.summary.split_whitespace().next().unwrap_or("");
    let verb = match action {
        "app_state" => "读取界面",
        "screenshot" => "截屏",
        "list_apps" => "列出应用",
        "click" => "点击目标",
        "type_text" => "输入文本",
        "set_value" => "写入字段",
        "key" => "按键",
        "scroll" => "滚动",
        "drag" => "移动",
        _ => "操作电脑",
    };
    match tone {
        ToolTone::Running => format!("正在{verb}"),
        ToolTone::Success => format!("已{verb}"),
        ToolTone::Failure => format!("{verb}失败"),
    }
}
