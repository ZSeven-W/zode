use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::AppCommand;
use zode_app_ui::{ReviewDraft, ReviewLineKind, ReviewPanel, ReviewSelection, ZodeTheme};
use zode_node_protocol::{DiffFile, DiffFileStatus, DiffSnapshot, SessionLocator};

const UNIFIED: &str =
    "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,3 @@\n one\n+two\n three\n";

#[derive(Default)]
struct PaintCapture {
    clips: Vec<Rect>,
    text_origins: Vec<(String, Point2D)>,
}

impl Painter for PaintCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, origin: Point2D) {
        let text = layout
            .runs()
            .iter()
            .map(|run| run.content.as_str())
            .collect::<String>();
        self.text_origins.push((text, origin));
    }
    fn clip_rect(&mut self, rect: Rect) {
        self.clips.push(rect);
    }
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color) {}
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
    fn stroke_svg_path(
        &mut self,
        _d: &str,
        _top_left: Point2D,
        _size: f32,
        _color: Color,
        _width: f32,
    ) {
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

#[test]
fn unified_hunks_project_line_numbers_and_change_kinds() {
    let lines = ReviewPanel::parse_unified(UNIFIED);
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0].kind, ReviewLineKind::Hunk);
    assert_eq!(lines[1].old_line, Some(1));
    assert_eq!(lines[1].new_line, Some(1));
    assert_eq!(lines[1].kind, ReviewLineKind::Context);
    assert_eq!(lines[2].old_line, None);
    assert_eq!(lines[2].new_line, Some(2));
    assert_eq!(lines[2].kind, ReviewLineKind::Addition);
    assert_eq!(lines[3].old_line, Some(2));
    assert_eq!(lines[3].new_line, Some(3));
}

#[test]
fn long_hunk_virtualization_keeps_one_overscan_line() {
    assert_eq!(
        ReviewPanel::visible_line_range(100, 200.0, 60.0, 20.0),
        9..13,
    );
}

#[test]
fn review_draft_tracks_selection_and_inline_comment_without_touching_git() {
    let mut draft = ReviewDraft::default();
    draft.select(ReviewSelection {
        path: "a.txt".into(),
        start_line: 2,
        end_line: 4,
    });
    draft.set_comment("Please keep this branch".into());

    assert_eq!(draft.selection().unwrap().start_line, 2);
    assert_eq!(draft.comment(), "Please keep this branch");
}

#[test]
fn open_file_action_routes_through_the_host_command() {
    let session = SessionLocator::new(Default::default(), "review");
    assert_eq!(
        ReviewPanel::open_file_command(session.clone(), "src/main.rs"),
        AppCommand::PreviewWorkspaceFile {
            session,
            relative_path: "src/main.rs".into(),
        },
    );
}

#[test]
fn file_list_is_clipped_and_adjacent_diff_rows_have_safe_leading() {
    let session = SessionLocator::new(Default::default(), "review");
    let snapshot = DiffSnapshot {
        session,
        files: vec![DiffFile {
            path: "crates/zode-app-ui/src/widgets/a-path-long-enough-to-cross-the-divider.rs"
                .into(),
            status: DiffFileStatus::Modified,
            additions: 1,
            deletions: 1,
        }],
        unified: concat!(
            "@@ -12,2 +12,2 @@\n",
            "-    let label = \"main\";\n",
            "+    let label = current_branch;\n",
        )
        .into(),
    };
    let mut painter = PaintCapture::default();

    ReviewPanel::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 700.0, 500.0),
        &snapshot,
        0.0,
        &ZodeTheme::light(),
    );

    assert!(
        painter.clips.contains(&Rect::xywh(0.0, 0.0, 210.0, 500.0)),
        "the file list needs its own clip before paths are painted",
    );
    let y = |needle: &str| {
        painter
            .text_origins
            .iter()
            .find_map(|(text, origin)| (text == needle).then_some(origin.y))
            .expect("diff line is painted")
    };
    assert!(
        y("    let label = current_branch;") - y("    let label = \"main\";") >= 24.0,
        "adjacent change rows need enough leading for the bundled CJK font",
    );
    assert!(
        (4.0..=6.0).contains(&(y("    let label = \"main\";") - 24.0)),
        "text origin must stay near the top of its 24px row",
    );
}
