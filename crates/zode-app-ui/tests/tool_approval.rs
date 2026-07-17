use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    demo_state, reduce_tool_command, AppCommand, ToolCommandOutcome, TranscriptItem,
    TranscriptState,
};
use zode_app_ui::{
    ApprovalAction, ApprovalCard, ToolCard, ToolTone, UsageChip, UsageDisplay, ZodeTheme,
};
use zode_node_protocol::{ApprovalDecision, SessionLocator, ToolCall, ToolStatus, UsageSnapshot};

fn tool(id: &str, name: &str, status: ToolStatus) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        status,
        summary: "summary".into(),
        detail: None,
    }
}

#[test]
fn approval_actions_map_to_the_three_protocol_decisions() {
    assert_eq!(
        ApprovalCard::decision(ApprovalAction::AllowOnce),
        ApprovalDecision::AllowOnce,
    );
    assert_eq!(
        ApprovalCard::decision(ApprovalAction::AllowAlways),
        ApprovalDecision::AllowAlways,
    );
    assert_eq!(
        ApprovalCard::decision(ApprovalAction::Deny),
        ApprovalDecision::Deny,
    );
}

#[test]
fn tool_cards_default_closed_for_read_create_and_open_for_mutation() {
    assert!(!ToolCard::default_expanded(&tool(
        "1",
        "read_file",
        ToolStatus::Running,
    )));
    assert!(!ToolCard::default_expanded(&tool(
        "2",
        "create_file",
        ToolStatus::Running,
    )));
    assert!(ToolCard::default_expanded(&tool(
        "3",
        "edit_file",
        ToolStatus::Running,
    )));
    assert!(ToolCard::default_expanded(&tool(
        "4",
        "delete_file",
        ToolStatus::Running,
    )));
    assert!(ToolCard::default_expanded(&tool(
        "5",
        "task_agent",
        ToolStatus::Running,
    )));
}

#[test]
fn tool_status_maps_to_running_success_and_failure_tones() {
    assert_eq!(
        ToolCard::tone(&tool("1", "read_file", ToolStatus::Running)),
        ToolTone::Running,
    );
    assert_eq!(
        ToolCard::tone(&tool("1", "read_file", ToolStatus::Completed)),
        ToolTone::Success,
    );
    assert_eq!(
        ToolCard::tone(&tool("1", "read_file", ToolStatus::Failed)),
        ToolTone::Failure,
    );
}

#[test]
fn expanded_state_is_keyed_by_tool_id() {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "tools");
    state.transcripts.insert(
        session.clone(),
        TranscriptState {
            items: vec![TranscriptItem::Tool(tool(
                "tool-1",
                "read_file",
                ToolStatus::Running,
            ))],
            ..TranscriptState::default()
        },
    );

    assert_eq!(
        reduce_tool_command(
            &mut state,
            AppCommand::SetToolExpanded {
                session: session.clone(),
                tool_id: "tool-1".into(),
                expanded: true,
            },
        ),
        ToolCommandOutcome::Applied,
    );
    assert_eq!(state.tool_expanded[&session].get("tool-1"), Some(&true));
}

#[test]
fn usage_display_includes_model_context_tokens_and_optional_cost() {
    let usage = UsageSnapshot {
        input_tokens: 1_200,
        output_tokens: 300,
        context_used: Some(0.423),
        cost_usd: Some(0.1234),
    };
    assert_eq!(
        UsageChip::display(Some("gpt-5.2"), &usage),
        UsageDisplay {
            model: "gpt-5.2".into(),
            context: "42%".into(),
            tokens: "1,500".into(),
            cost: "$0.1234".into(),
        },
    );

    let no_price = UsageSnapshot {
        cost_usd: None,
        ..usage
    };
    assert_eq!(UsageChip::display(None, &no_price).cost, "n/a");
}

#[test]
fn card_controls_share_their_visual_centerline() {
    let theme = ZodeTheme::light();
    let mut painter = TextCapture::default();
    let approval_rect = Rect::xywh(0.0, 0.0, 260.0, 64.0);
    ApprovalCard::paint(&mut painter, approval_rect, "write_file", &theme);
    for button in ApprovalCard::button_layout(approval_rect) {
        assert_close(
            painter.center_y(button.label),
            button.rect.origin.y + button.rect.size.y / 2.0,
            1.0,
        );
    }

    let chip = Rect::xywh(0.0, 80.0, 300.0, 24.0);
    UsageChip::paint(
        &mut painter,
        chip,
        Some("gpt-5.2"),
        &UsageSnapshot {
            input_tokens: 100,
            output_tokens: 20,
            context_used: Some(0.2),
            cost_usd: None,
        },
        &theme,
    );
    assert_close(
        painter.center_y("gpt-5.2 · 20% · 120 tok · n/a"),
        chip.origin.y + chip.size.y / 2.0,
        1.0,
    );

    let tool_rect = Rect::xywh(0.0, 120.0, 260.0, 64.0);
    let painted_tool = tool("centered", "read_file", ToolStatus::Running);
    ToolCard::paint(&mut painter, tool_rect, &painted_tool, false, &theme);
    assert_close(
        painter.center_y("正在读取"),
        tool_rect.origin.y + tool_rect.size.y.min(35.0) / 2.0,
        1.0,
    );
}

#[derive(Default)]
struct TextCapture {
    calls: Vec<(String, Point2D, f32)>,
}

impl TextCapture {
    fn center_y(&self, text: &str) -> f32 {
        let (_, origin, size) = self
            .calls
            .iter()
            .find(|(candidate, _, _)| candidate == text)
            .unwrap_or_else(|| panic!("missing text call: {text}"));
        origin.y + size / 2.0
    }
}

impl Painter for TextCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, origin: Point2D) {
        let run = layout.runs().first().expect("single-line text");
        self.calls
            .push((run.content.clone(), origin, run.font_size));
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color) {}
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
    fn stroke_svg_path(
        &mut self,
        _path: &str,
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

fn assert_close(actual: f32, expected: f32, tolerance: f32) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {actual} within {tolerance} of {expected}"
    );
}
