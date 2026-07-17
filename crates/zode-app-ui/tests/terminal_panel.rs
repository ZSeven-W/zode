use zode_app_model::{demo_state, reduce_terminal_command, AppCommand, TerminalCommandOutcome};
use zode_app_ui::{CellPosition, TerminalColor, TerminalGrid, TerminalPanel, TerminalSelection};

#[test]
fn sgr_color_and_clear_screen_update_the_grid() {
    let mut grid = TerminalGrid::new(20, 4);
    grid.feed(b"\x1b[31mred");
    assert_eq!(grid.cell(0, 0).foreground, TerminalColor::Red);

    grid.feed(b"\x1b[0m\x1b[2Jok");
    assert_eq!(grid.line(0).plain_text(), "ok");
    assert_eq!(grid.cell(0, 0).foreground, TerminalColor::Default);
}

#[test]
fn cursor_position_newline_backspace_and_scroll_region_are_supported() {
    let mut grid = TerminalGrid::new(8, 3);
    grid.feed(b"one\r\ntwo\r\nthree\x08!");
    assert_eq!(grid.line(0).plain_text(), "one");
    assert_eq!(grid.line(1).plain_text(), "two");
    assert_eq!(grid.line(2).plain_text(), "thre!");

    grid.feed(b"\x1b[1;2H@");
    assert_eq!(grid.cell(0, 1).character, '@');
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
}

#[test]
fn focus_and_scroll_are_reduced_without_platform_state() {
    let mut state = demo_state();
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
