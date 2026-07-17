use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{demo_state, reduce_terminal_command, AppCommand, TerminalCommandOutcome};
use zode_app_ui::{
    CellPosition, Key, KeyEvent, Modifiers, TerminalColor, TerminalGrid, TerminalPanel,
    TerminalPanelController, TerminalSelection, ZodeTheme,
};

#[derive(Default)]
struct PaintCapture {
    fills: Vec<(Rect, Color)>,
    texts: Vec<String>,
    text_colors: Vec<(String, (u8, u8, u8))>,
    strokes: Vec<(Rect, Color)>,
}

impl Painter for PaintCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.fills.push((rect, color));
    }
    fn stroke_rect(&mut self, rect: Rect, color: Color, _width: f32) {
        self.strokes.push((rect, color));
    }
    fn draw_text(&mut self, layout: &TextLayout, _origin: Point2D) {
        let text = layout
            .runs()
            .iter()
            .map(|run| run.content.as_str())
            .collect::<String>();
        if let Some(run) = layout.runs().first() {
            self.text_colors
                .push((text.clone(), (run.color.r(), run.color.g(), run.color.b())));
        }
        self.texts.push(text);
    }
    fn clip_rect(&mut self, _rect: Rect) {}
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
fn terminal_panel_sgr_color_and_clear_screen_update_the_grid() {
    let mut grid = TerminalGrid::new(20, 4);
    grid.feed(b"\x1b[31mred");
    assert_eq!(grid.cell(0, 0).foreground, TerminalColor::Red);

    grid.feed(b"\x1b[0m\x1b[2Jok");
    assert_eq!(grid.line(0).plain_text(), "ok");
    assert_eq!(grid.cell(0, 0).foreground, TerminalColor::Default);
}

#[test]
fn terminal_panel_cursor_position_newline_backspace_and_scroll_region_are_supported() {
    let mut grid = TerminalGrid::new(8, 3);
    grid.feed(b"one\r\ntwo\r\nthree\x08!");
    assert_eq!(grid.line(0).plain_text(), "one");
    assert_eq!(grid.line(1).plain_text(), "two");
    assert_eq!(grid.line(2).plain_text(), "thre!");

    grid.feed(b"\x1b[1;2H@");
    assert_eq!(grid.cell(0, 1).character, '@');
}

#[test]
fn terminal_panel_full_width_line_feed_advances_only_once() {
    let mut grid = TerminalGrid::new(3, 3);
    grid.feed(b"abc\nx");

    assert_eq!(grid.line(1).plain_text(), "  x");
    assert_eq!(grid.line(2).plain_text(), "");
}

#[test]
fn terminal_panel_screen_access_stays_screen_relative_and_saved_lines_can_be_erased() {
    let mut grid = TerminalGrid::new(5, 2);
    grid.feed(b"one\r\ntwo\r\nthree");

    assert_eq!(grid.line(0).plain_text(), "two");
    assert_eq!(grid.line_count(), 3);

    grid.feed(b"\x1b[3J");
    assert_eq!(grid.line_count(), 2);
}

#[test]
fn terminal_panel_grid_resize_updates_geometry_without_reflowing_lines() {
    let mut grid = TerminalGrid::new(5, 3);
    grid.feed(b"one\r\ntwo\r\nthree");

    grid.resize(7, 4);

    assert_eq!(grid.size(), (7, 4));
    assert_eq!(grid.line(2).plain_text(), "three");

    grid.resize(5, 2);
    grid.feed(b"!");
    assert_eq!(grid.size(), (5, 2));
    assert_eq!(grid.line(0).plain_text(), "two");
    assert_eq!(grid.line(1).plain_text(), "thre!");
    assert_eq!(grid.line_count(), 3);
}

#[test]
fn terminal_panel_virtualizes_and_copies_the_selected_text() {
    assert_eq!(
        TerminalPanel::visible_line_range(100, 200.0, 60.0, 20.0),
        9..13,
    );
    let mut grid = TerminalGrid::new(12, 2);
    grid.feed(b"hello world");
    let selection = TerminalSelection {
        start: CellPosition { row: 0, col: 1 },
        end: CellPosition { row: 0, col: 5 },
    };
    assert_eq!(TerminalPanel::copy_selection(&grid, selection), "ello");

    assert_eq!(
        TerminalPanel::visible_line_range(1_000_000_000, 200.0, 60.0, 20.0),
        9..13,
    );
    assert_eq!(
        TerminalPanel::visible_line_range(5, 10_000.0, 40.0, 20.0),
        4..5,
    );
    assert_eq!(TerminalPanel::tail_offset(5, 40.0), 60.0);
}

#[test]
fn terminal_panel_scrollback_evicts_oldest_lines_at_a_fixed_limit() {
    let mut grid = TerminalGrid::new(4, 2);
    for _ in 0..TerminalGrid::scrollback_limit() + 32 {
        grid.feed(b"x\r\n");
    }

    assert_eq!(
        grid.line_count(),
        TerminalGrid::scrollback_limit() + grid.size().1
    );
    grid.resize(8, 3);
    assert!(grid.line_count() <= TerminalGrid::scrollback_limit() + grid.size().1);
}

#[test]
fn terminal_panel_focus_and_scroll_are_reduced_without_platform_state() {
    let mut state = demo_state();
    assert_eq!(
        reduce_terminal_command(&mut state, AppCommand::OpenTerminal),
        TerminalCommandOutcome::Applied,
    );
    assert!(state.terminal.open);
    assert_eq!(state.shell.page, zode_app_model::ShellPage::Terminal);
    assert_eq!(
        reduce_terminal_command(&mut state, AppCommand::SetTerminalFocus(true)),
        TerminalCommandOutcome::Applied,
    );
    assert_eq!(
        reduce_terminal_command(
            &mut state,
            AppCommand::SetTerminalScroll {
                offset: 80.0,
                follow_tail: false,
            },
        ),
        TerminalCommandOutcome::Applied,
    );
    assert!(state.terminal.focused);
    assert_eq!(state.terminal.scroll_offset, 80.0);
    assert!(!state.terminal.follow_tail);
}

#[test]
fn terminal_panel_controller_routes_selection_copy_scroll_focus_and_terminal_input() {
    let rect = Rect::xywh(0.0, 0.0, 120.0, 20.0);
    let mut grid = TerminalGrid::new(12, 2);
    grid.feed(b"hello world");
    let mut controller = TerminalPanelController::default();
    let mut state = demo_state().terminal;

    assert_eq!(
        controller.pointer_down(rect, Point2D::new(17.0, 10.0), &grid, 0.0),
        Some(AppCommand::SetTerminalFocus(true)),
    );
    assert!(controller.pointer_move(rect, Point2D::new(41.0, 10.0), &grid, 0.0));
    controller.pointer_up();
    assert_eq!(
        controller.copy_command(&grid),
        Some(AppCommand::CopyText("ello".into())),
    );

    grid.feed(b"\r\none\r\ntwo\r\nthree");
    assert_eq!(
        controller.scroll_command(&state, &grid, 20.0, 20.0),
        AppCommand::SetTerminalScroll {
            offset: 20.0,
            follow_tail: false,
        },
    );

    let terminal = zode_node_protocol::TerminalId::new();
    state.active_id = Some(terminal);
    state.focused = true;
    assert_eq!(
        controller.key_command(
            &state,
            &KeyEvent {
                key: Key::Character("x".into()),
                modifiers: Modifiers::NONE,
                pressed: true,
            },
        ),
        Some(AppCommand::WriteTerminal {
            id: terminal,
            bytes: b"x".to_vec(),
        }),
    );

    for (letter, byte) in [("c", 0x03), ("d", 0x04), ("l", 0x0c), ("z", 0x1a)] {
        assert_eq!(
            controller.key_command(
                &state,
                &KeyEvent {
                    key: Key::Character(letter.into()),
                    modifiers: Modifiers::CONTROL,
                    pressed: true,
                },
            ),
            Some(AppCommand::WriteTerminal {
                id: terminal,
                bytes: vec![byte],
            }),
        );
    }
    assert!(TerminalPanelController::is_copy_shortcut(&KeyEvent {
        key: Key::Character("c".into()),
        modifiers: Modifiers::SUPER,
        pressed: true,
    }));
    assert!(TerminalPanelController::is_copy_shortcut(&KeyEvent {
        key: Key::Character("c".into()),
        modifiers: Modifiers::CONTROL | Modifiers::SHIFT,
        pressed: true,
    }));
    assert!(!TerminalPanelController::is_copy_shortcut(&KeyEvent {
        key: Key::Character("c".into()),
        modifiers: Modifiers::CONTROL,
        pressed: true,
    }));
}

#[test]
fn terminal_panel_paints_scrolled_lines_focus_and_mobile_unavailable_message() {
    let rect = Rect::xywh(0.0, 0.0, 240.0, 40.0);
    let theme = ZodeTheme::light();
    let mut state = demo_state().terminal;
    state.focused = true;
    state.scroll_offset = 60.0;
    let mut grid = TerminalGrid::new(8, 3);
    grid.feed(b"zero\r\none\r\ntwo\r\nthree\r\nfour");
    let mut painter = PaintCapture::default();

    TerminalPanel::paint(&mut painter, rect, &grid, &state, None, &theme);

    assert!(!painter.texts.iter().any(|text| text == "zero"));
    assert!(painter.texts.iter().any(|text| text == "four"));
    assert!(painter
        .strokes
        .iter()
        .any(|(stroke, color)| *stroke == rect && *color == theme.zode_purple));

    state.unavailable_reason = Some("Terminal is not available on mobile".into());
    painter.texts.clear();
    TerminalPanel::paint(&mut painter, rect, &grid, &state, None, &theme);
    assert_eq!(
        painter.texts,
        vec!["Terminal is not available on mobile".to_owned()]
    );
}

#[test]
fn terminal_panel_paints_indexed_sgr_backgrounds_and_styled_trailing_spaces() {
    let rect = Rect::xywh(0.0, 0.0, 80.0, 20.0);
    let theme = ZodeTheme::light();
    let state = demo_state().terminal;
    let mut grid = TerminalGrid::new(8, 1);
    grid.feed(b"\x1b[48;5;21;38;5;196mA ");
    let mut painter = PaintCapture::default();

    TerminalPanel::paint(&mut painter, rect, &grid, &state, None, &theme);

    assert!(painter.texts.iter().any(|text| text == "A "));
    assert!(painter
        .fills
        .iter()
        .any(|(fill, color)| { fill.size.x == 16.0 && *color == Color::rgb_u8(0, 0, 255) }));
    assert!(painter
        .text_colors
        .iter()
        .any(|(text, color)| text == "A " && *color == (255, 0, 0)));
}
