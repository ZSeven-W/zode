use jian_widgets::{Painter, Point2D, Rect};
use zode_app_model::FileArtifact;

use crate::{Card, RectExt, SemanticIcon, ZodeTheme};

use super::draw_text;

pub(super) const HEIGHT: f32 = 64.0;

/// Broad file-type buckets driving both the card's type icon and its
/// "category · EXT" label. Deliberately coarse (a handful of buckets, not
/// one label per extension) - `Other` is the honest answer for anything not
/// recognized, not a parsing failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileKind {
    Document,
    Code,
    Data,
    Image,
    Other,
}

impl FileKind {
    fn from_extension(extension: &str) -> Self {
        match extension.to_ascii_lowercase().as_str() {
            "md" | "mdx" | "txt" | "rst" | "adoc" => Self::Document,
            "rs" | "py" | "js" | "jsx" | "ts" | "tsx" | "go" | "java" | "c" | "h" | "hpp"
            | "cpp" | "cc" | "rb" | "swift" | "kt" | "kts" | "php" | "cs" | "scala" | "lua"
            | "sh" | "sql" => Self::Code,
            "json" | "yaml" | "yml" | "toml" | "csv" | "xml" | "ini" => Self::Data,
            "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "ico" => Self::Image,
            _ => Self::Other,
        }
    }

    fn icon(self) -> SemanticIcon {
        match self {
            Self::Document | Self::Other => SemanticIcon::FileText,
            Self::Code => SemanticIcon::FileCode,
            Self::Data => SemanticIcon::FileData,
            Self::Image => SemanticIcon::FileImage,
        }
    }

    fn category_label(self) -> &'static str {
        match self {
            Self::Document => "文档",
            Self::Code => "代码",
            Self::Data => "数据",
            Self::Image => "图片",
            Self::Other => "文件",
        }
    }
}

fn path_extension(path: &str) -> Option<&str> {
    std::path::Path::new(path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|extension| !extension.is_empty())
}

fn file_kind(path: &str) -> FileKind {
    path_extension(path).map_or(FileKind::Other, FileKind::from_extension)
}

/// "文档 · MD" / "代码 · RS" style badge: the category plus the raw
/// extension, uppercased. Falls back to the bare category when the path has
/// no extension.
fn type_label(path: &str, kind: FileKind) -> String {
    match path_extension(path) {
        Some(extension) => format!(
            "{} · {}",
            kind.category_label(),
            extension.to_ascii_uppercase()
        ),
        None => kind.category_label().to_string(),
    }
}

const OPEN_WITH_LABEL: &str = "打开方式";

pub(super) fn paint(painter: &mut dyn Painter, rect: Rect, file: &FileArtifact, theme: &ZodeTheme) {
    Card::paint(painter, rect, 12.0, false, theme);
    let kind = file_kind(&file.path);
    paint_icon_tile(painter, rect, kind.icon(), theme);
    draw_text(
        painter,
        &file.summary,
        Point2D::new(rect.origin.x + 64.0, rect.origin.y + 14.0),
        13.0,
        600,
        theme.tokens.foreground,
    );
    let open_with_width = paint_open_with(painter, rect, theme);
    paint_subtitle(painter, rect, file, kind, open_with_width, theme);
    if let Some(change) = file.change_summary.as_deref() {
        paint_change_summary(painter, rect, change, theme);
    }
}

/// Type label plus path, clipped so a long path can never run under the
/// right-aligned "打开方式" affordance regardless of how long it is - the
/// existing card never truncated `file.path`, so this keeps that path
/// visually safe without changing its content.
fn paint_subtitle(
    painter: &mut dyn Painter,
    rect: Rect,
    file: &FileArtifact,
    kind: FileKind,
    reserved_right: f32,
    theme: &ZodeTheme,
) {
    let left = rect.origin.x + 64.0;
    let right = (rect.max_x() - 14.0 - reserved_right - 10.0).max(left);
    painter.save();
    painter.clip_rect(Rect::xywh(
        left,
        rect.origin.y + 34.0,
        (right - left).max(0.0),
        16.0,
    ));
    draw_text(
        painter,
        &format!("{} › {}", type_label(&file.path, kind), file.path),
        Point2D::new(left, rect.origin.y + 36.0),
        11.0,
        400,
        theme.tokens.muted_foreground,
    );
    painter.restore();
}

/// Right-aligned "打开方式" affordance. Clicking anywhere on the card -
/// including this label - already dispatches the same open command as
/// clicking the file name (see `ThreadTranscript::command_for_widget`'s
/// single hit region for `TranscriptItem::FileArtifact`), so this paints a
/// visual cue only; it does not register a separate widget id. Returns the
/// horizontal space it occupies so the subtitle can clip clear of it.
fn paint_open_with(painter: &mut dyn Painter, rect: Rect, theme: &ZodeTheme) -> f32 {
    let chevron_size = 10.0;
    let gap = 4.0;
    let label_width = painter.measure_text_weighted(OPEN_WITH_LABEL, 11.0, 500);
    let right = rect.max_x() - 14.0;
    let chevron_x = right - chevron_size;
    let label_x = chevron_x - gap - label_width;
    draw_text(
        painter,
        OPEN_WITH_LABEL,
        Point2D::new(label_x, rect.origin.y + 36.0),
        11.0,
        500,
        theme.tokens.muted_foreground,
    );
    painter.stroke_svg_path(
        SemanticIcon::ChevronDown.path(),
        Point2D::new(chevron_x, rect.origin.y + 37.0),
        chevron_size,
        theme.tokens.muted_foreground,
        SemanticIcon::ChevronDown.stroke_width(),
    );
    label_width + gap + chevron_size
}

pub(super) fn paint_icon_tile(
    painter: &mut dyn Painter,
    rect: Rect,
    icon: SemanticIcon,
    theme: &ZodeTheme,
) {
    let tile = Rect::xywh(rect.origin.x + 12.0, rect.origin.y + 12.0, 40.0, 40.0);
    painter.fill_round_rect(tile, 10.0, theme.tokens.muted.with_alpha(0.72));
    painter.stroke_svg_path(
        icon.path(),
        Point2D::new(tile.origin.x + 11.0, tile.origin.y + 11.0),
        18.0,
        theme.tokens.muted_foreground,
        icon.stroke_width(),
    );
}

fn paint_change_summary(painter: &mut dyn Painter, rect: Rect, change: &str, theme: &ZodeTheme) {
    let y = rect.origin.y + 14.0;
    let right = rect.max_x() - 14.0;
    if let Some((additions, deletions)) = split_change_summary(change) {
        let additions_width = painter.measure_text_weighted(additions, 11.0, 500);
        let deletions_width = painter.measure_text_weighted(deletions, 11.0, 500);
        let x = (right - additions_width - 5.0 - deletions_width).max(rect.origin.x + 180.0);
        draw_text(
            painter,
            additions,
            Point2D::new(x, y),
            11.0,
            500,
            theme.success,
        );
        draw_text(
            painter,
            deletions,
            Point2D::new(x + additions_width + 5.0, y),
            11.0,
            500,
            theme.tokens.destructive,
        );
        return;
    }

    let x = (right - painter.measure_text_weighted(change, 11.0, 500)).max(rect.origin.x + 180.0);
    draw_text(
        painter,
        change,
        Point2D::new(x, y),
        11.0,
        500,
        theme.success,
    );
}

fn split_change_summary(change: &str) -> Option<(&str, &str)> {
    let mut parts = change.split_whitespace();
    let additions = parts.next()?;
    let deletions = parts.next()?;
    if parts.next().is_some() || !valid_delta(additions, '+') || !valid_delta(deletions, '-') {
        return None;
    }
    Some((additions, deletions))
}

fn valid_delta(value: &str, sign: char) -> bool {
    value.strip_prefix(sign).is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}
