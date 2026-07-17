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
        let (label, icon) = action_presentation(&tool.name, tone);
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
            label,
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
        if expanded {
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

fn action_presentation(name: &str, tone: ToolTone) -> (&'static str, SemanticIcon) {
    let name = name.to_ascii_lowercase();
    if [
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
    } else if name.contains("agent") || name.contains("task") {
        (
            match tone {
                ToolTone::Running => "正在运行子任务",
                ToolTone::Success => "已运行子任务",
                ToolTone::Failure => "子任务失败",
            },
            SemanticIcon::Sparkles,
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
    }
}
