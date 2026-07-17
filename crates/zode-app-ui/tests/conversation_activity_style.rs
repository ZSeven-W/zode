use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{demo_state, ActivityEntry, TranscriptItem, TranscriptState};
use zode_app_ui::{SemanticIcon, ThreadTranscript, ToolCard, ZodeTheme};
use zode_node_protocol::{SessionLocator, ToolCall, ToolStatus};

#[derive(Debug, Clone)]
struct TextCall {
    content: String,
    font_size: f32,
}

#[derive(Default)]
struct PaintCapture {
    text: Vec<TextCall>,
    round_fills: usize,
    round_strokes: usize,
    svg_paths: Vec<String>,
}

impl PaintCapture {
    fn font_size(&self, content: &str) -> f32 {
        self.text
            .iter()
            .find(|call| call.content == content)
            .unwrap_or_else(|| panic!("missing text call: {content}"))
            .font_size
    }

    fn contains(&self, content: &str) -> bool {
        self.text.iter().any(|call| call.content == content)
    }
}

impl Painter for PaintCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, _origin: Point2D) {
        for run in layout.runs() {
            self.text.push(TextCall {
                content: run.content.clone(),
                font_size: run.font_size,
            });
        }
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color) {
        self.round_fills += 1;
    }
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {
        self.round_strokes += 1;
    }
    fn stroke_svg_path(
        &mut self,
        path: &str,
        _top_left: Point2D,
        _size: f32,
        _color: Color,
        _width: f32,
    ) {
        self.svg_paths.push(path.to_owned());
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

fn tool(name: &str, summary: &str) -> ToolCall {
    ToolCall {
        id: name.into(),
        name: name.into(),
        status: ToolStatus::Completed,
        summary: summary.into(),
        detail: None,
    }
}

#[test]
fn tool_rows_are_lightweight_and_translate_common_actions() {
    let theme = ZodeTheme::light();
    for (name, expected) in [
        ("read_file", "已读取"),
        ("list_files", "已读取"),
        ("search_code", "已读取"),
        ("shell_exec", "运行了命令"),
        ("edit_file", "已编辑"),
        ("write_file", "已编辑"),
    ] {
        let mut painter = PaintCapture::default();
        ToolCard::paint(
            &mut painter,
            Rect::xywh(0.0, 0.0, 420.0, 60.0),
            &tool(name, "操作详情"),
            true,
            &theme,
        );

        assert!(painter.contains(expected), "{name} should paint {expected}");
        assert_eq!(painter.font_size(expected), 14.0);
        assert_eq!(painter.font_size("操作详情"), 13.0);
        assert_eq!(painter.round_fills, 0, "activity rows have no filled dot");
        assert_eq!(painter.round_strokes, 0, "lightweight rows have no border");
        assert_eq!(painter.svg_paths.len(), 1);
    }
}

#[test]
fn thinking_status_and_activity_paint_as_neutral_action_rows() {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "activity-style");
    state.current_session = Some(session.clone());
    state.transcripts.insert(
        session,
        TranscriptState {
            items: vec![
                TranscriptItem::Thinking("正在思考".into()),
                TranscriptItem::ActivityGroup(vec![ActivityEntry {
                    id: "read".into(),
                    title: "已读取文件".into(),
                    detail: Some("3 个文件".into()),
                    completed: true,
                }]),
                TranscriptItem::Status {
                    code: "ready".into(),
                    message: "上下文已自动压缩".into(),
                },
            ],
            follow_tail: false,
            ..TranscriptState::default()
        },
    );
    let mut painter = PaintCapture::default();

    ThreadTranscript::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 736.0, 500.0),
        &state,
        &ZodeTheme::light(),
    );

    assert!(!painter.contains("思考"));
    assert!(!painter.contains("活动"));
    assert_eq!(painter.font_size("正在思考"), 14.0);
    assert_eq!(painter.font_size("已读取文件"), 14.0);
    assert_eq!(painter.font_size("3 个文件"), 13.0);
    assert_eq!(painter.font_size("上下文已自动压缩"), 14.0);
    assert_eq!(painter.round_fills, 0);
    assert_eq!(painter.round_strokes, 0);
    assert_eq!(painter.svg_paths.len(), 3);
    assert!(painter
        .svg_paths
        .iter()
        .any(|path| path == SemanticIcon::Sparkles.path()));
    assert!(painter
        .svg_paths
        .iter()
        .any(|path| path == SemanticIcon::FileText.path()));
    assert!(painter
        .svg_paths
        .iter()
        .any(|path| path == SemanticIcon::Check.path()));
}
