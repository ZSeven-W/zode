use accesskit::Action;
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase as WinitTouchPhase},
    keyboard::{Key as WinitKey, ModifiersState, NamedKey},
    window::Theme as WinitTheme,
};
use zode_app::event_map::{
    composer_outcome_command, is_paste_shortcut_for, map_ime, map_ime_input, map_ime_input_owned,
    map_key, map_keyboard, map_pointer_button, map_pointer_move, map_system_theme, map_touch,
    map_wheel, route_key_event, terminal_shortcut_command, InputRoute, ShortcutPlatform,
};
use zode_app::input_dispatch::{
    ime_allowed_for_focus, settings_scroll_delta_for_action, settings_scroll_delta_for_key,
    ScrollTouchOutcome, ScrollTouchTracker,
};
use zode_app_model::{AppCommand, AttachmentMetadata, SystemTheme};
use zode_app_ui::{
    ComposerOutcome, ComposerSubmission, FocusDirection, ImeEvent, Key, KeyEvent, Modifiers,
    PointerButton, PointerEvent, PointerEventKind, TouchEvent, TouchPhase, UnifiedInputEvent,
    WheelDeltaMode, WheelEvent,
};
use zode_node_protocol::UserContent;

#[test]
fn named_and_character_keys_map_to_platform_neutral_events() {
    assert_eq!(
        map_key(
            &WinitKey::Named(NamedKey::PageDown),
            ModifiersState::empty(),
            true,
        ),
        Some(KeyEvent {
            key: Key::PageDown,
            modifiers: Modifiers::NONE,
            pressed: true,
        }),
    );
    assert_eq!(
        map_key(
            &WinitKey::Named(NamedKey::Enter),
            ModifiersState::SHIFT,
            true,
        ),
        Some(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::SHIFT,
            pressed: true,
        }),
    );
    assert_eq!(
        map_key(
            &WinitKey::Character("你".into()),
            ModifiersState::empty(),
            true,
        ),
        Some(KeyEvent {
            key: Key::Character("你".into()),
            modifiers: Modifiers::NONE,
            pressed: true,
        }),
    );
    assert_eq!(
        map_key(
            &WinitKey::Named(NamedKey::F12),
            ModifiersState::empty(),
            true,
        ),
        None,
    );
}

#[test]
fn settings_keyboard_and_accessibility_scroll_share_page_sized_deltas() {
    let height = 400.0;
    assert_eq!(
        settings_scroll_delta_for_key(&Key::PageDown, height),
        Some(340.0)
    );
    assert_eq!(
        settings_scroll_delta_for_key(&Key::PageUp, height),
        Some(-340.0)
    );
    assert_eq!(
        settings_scroll_delta_for_key(&Key::Home, height),
        Some(-f32::MAX)
    );
    assert_eq!(
        settings_scroll_delta_for_key(&Key::End, height),
        Some(f32::MAX)
    );
    assert_eq!(
        settings_scroll_delta_for_action(Action::ScrollDown, height),
        Some(340.0)
    );
    assert_eq!(
        settings_scroll_delta_for_action(Action::ScrollUp, height),
        Some(-340.0)
    );
}

#[test]
fn scroll_touch_pan_scrolls_without_also_becoming_a_tap() {
    let content = jian_widgets::Rect::xywh(0.0, 0.0, 300.0, 400.0);
    let mut tracker = ScrollTouchTracker::default();
    let touch = |y, phase| TouchEvent {
        id: 7,
        position: jian_widgets::Point2D::new(20.0, y),
        phase,
    };

    assert_eq!(
        tracker.handle(touch(200.0, TouchPhase::Started), content),
        ScrollTouchOutcome::Captured
    );
    assert_eq!(
        tracker.handle(touch(150.0, TouchPhase::Moved), content),
        ScrollTouchOutcome::Scroll(50.0)
    );
    assert_eq!(
        tracker.handle(touch(150.0, TouchPhase::Ended), content),
        ScrollTouchOutcome::Captured
    );

    assert_eq!(
        tracker.handle(touch(80.0, TouchPhase::Started), content),
        ScrollTouchOutcome::Captured
    );
    assert_eq!(
        tracker.handle(touch(80.0, TouchPhase::Ended), content),
        ScrollTouchOutcome::Tap(jian_widgets::Point2D::new(20.0, 80.0))
    );
}

#[test]
fn ime_preedit_uses_the_cursor_end_and_preserves_lifecycle() {
    assert_eq!(map_ime(&Ime::Enabled), ImeEvent::Start);
    assert_eq!(
        map_ime(&Ime::Preedit("拼音".into(), Some((0, "拼".len())))),
        ImeEvent::Update {
            text: "拼音".into(),
            cursor: Some("拼".len()),
        },
    );
    assert_eq!(
        map_ime(&Ime::Commit("中文".into())),
        ImeEvent::Commit("中文".into()),
    );
    assert_eq!(map_ime(&Ime::Disabled), ImeEvent::End);
}

#[test]
fn composer_outcomes_map_to_controller_commands() {
    let mut send = ComposerOutcome::Send("hello".into());
    assert_eq!(
        composer_outcome_command(&mut send),
        Some(AppCommand::Submit(vec![UserContent::Text {
            text: "hello".into(),
        }])),
    );
    let ComposerOutcome::Send(submission) = send else {
        unreachable!();
    };
    assert!(submission.content.is_empty());

    let mut steer = ComposerOutcome::Steer("redirect".into());
    assert_eq!(
        composer_outcome_command(&mut steer),
        Some(AppCommand::Steer(vec![UserContent::Text {
            text: "redirect".into(),
        }])),
    );
    let ComposerOutcome::Steer(submission) = steer else {
        unreachable!();
    };
    assert!(submission.content.is_empty());

    let mut stop = ComposerOutcome::Stop;
    assert_eq!(
        composer_outcome_command(&mut stop),
        Some(AppCommand::Interrupt),
    );
    let mut edited = ComposerOutcome::Edited;
    assert_eq!(composer_outcome_command(&mut edited), None,);
}

#[test]
fn composer_command_moves_payload_but_preserves_attachment_metadata() {
    let attachment = AttachmentMetadata {
        id: "attachment-1".into(),
        path: None,
        display_name: "shot.png".into(),
        media_type: "image/png".into(),
        width: Some(640),
        height: Some(360),
        byte_len: 1_024,
    };
    let mut outcome = ComposerOutcome::Send(ComposerSubmission {
        content: vec![UserContent::Image {
            mime_type: "image/png".into(),
            data_base64: "cGF5bG9hZA==".into(),
            display_name: "shot.png".into(),
        }],
        attachments: vec![attachment.clone()],
    });

    let command = composer_outcome_command(&mut outcome).expect("send command");

    assert!(matches!(command, AppCommand::Submit(content) if content.len() == 1));
    let ComposerOutcome::Send(submission) = outcome else {
        unreachable!();
    };
    assert!(submission.content.is_empty());
    assert_eq!(submission.attachments, [attachment]);
}

#[test]
fn terminal_shortcut_routes_to_the_terminal_page() {
    assert_eq!(
        terminal_shortcut_command(&KeyEvent {
            key: Key::Character("`".into()),
            modifiers: Modifiers::SUPER,
            pressed: true,
        }),
        Some(AppCommand::OpenTerminal),
    );
}

#[test]
fn host_maps_pointer_move_and_button_phases_to_one_vocabulary() {
    assert_eq!(
        map_pointer_move(PhysicalPosition::new(40.0, 60.0), 2.0),
        UnifiedInputEvent::Pointer(PointerEvent {
            position: jian_widgets::Point2D::new(20.0, 30.0),
            kind: PointerEventKind::Move,
            button: None,
        }),
    );
    let cached = jian_widgets::Point2D::new(17.0, 23.0);
    assert_eq!(
        map_pointer_button(MouseButton::Left, ElementState::Pressed, cached),
        UnifiedInputEvent::Pointer(PointerEvent {
            position: cached,
            kind: PointerEventKind::Press,
            button: Some(PointerButton::Primary),
        }),
    );
    assert_eq!(
        map_pointer_button(MouseButton::Left, ElementState::Released, cached),
        UnifiedInputEvent::Pointer(PointerEvent {
            position: cached,
            kind: PointerEventKind::Release,
            button: Some(PointerButton::Primary),
        }),
    );
}

#[test]
fn host_maps_touch_pixel_and_line_wheel_without_losing_units() {
    assert_eq!(
        map_touch(
            9,
            PhysicalPosition::new(30.0, 50.0),
            WinitTouchPhase::Moved,
            2.0,
        ),
        UnifiedInputEvent::Touch(TouchEvent {
            id: 9,
            position: jian_widgets::Point2D::new(15.0, 25.0),
            phase: TouchPhase::Moved,
        }),
    );
    assert_eq!(
        map_wheel(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(8.0, -12.0)),
            2.0,
        ),
        UnifiedInputEvent::Wheel(WheelEvent {
            delta_x: 4.0,
            delta_y: -6.0,
            mode: WheelDeltaMode::Pixel,
        }),
    );
    assert_eq!(
        map_wheel(MouseScrollDelta::LineDelta(1.0, -3.0), 2.0),
        UnifiedInputEvent::Wheel(WheelEvent {
            delta_x: 1.0,
            delta_y: -3.0,
            mode: WheelDeltaMode::Line,
        }),
    );
}

#[test]
fn keyboard_and_ime_mappers_also_emit_unified_events() {
    assert_eq!(
        map_keyboard(&WinitKey::Named(NamedKey::Tab), ModifiersState::SHIFT, true,),
        Some(UnifiedInputEvent::Keyboard(KeyEvent {
            key: Key::Tab,
            modifiers: Modifiers::SHIFT,
            pressed: true,
        })),
    );
    assert_eq!(
        map_ime_input(&Ime::Commit("中文".into())),
        UnifiedInputEvent::Ime(ImeEvent::Commit("中文".into())),
    );
    assert_eq!(
        map_ime_input_owned(Ime::Commit("中文".into())),
        UnifiedInputEvent::Ime(ImeEvent::Commit("中文".into())),
    );
}

#[test]
fn tab_routes_through_focus_tree_but_plain_terminal_tab_stays_in_pty() {
    let plain_tab = KeyEvent {
        key: Key::Tab,
        modifiers: Modifiers::NONE,
        pressed: true,
    };
    let reverse_tab = KeyEvent {
        key: Key::Tab,
        modifiers: Modifiers::SHIFT,
        pressed: true,
    };

    assert_eq!(route_key_event(&plain_tab, true), InputRoute::TerminalPty);
    assert_eq!(
        route_key_event(&reverse_tab, true),
        InputRoute::MoveFocus(FocusDirection::Backward),
    );
    assert_eq!(
        route_key_event(&plain_tab, false),
        InputRoute::MoveFocus(FocusDirection::Forward),
    );
    assert_eq!(
        route_key_event(&reverse_tab, false),
        InputRoute::MoveFocus(FocusDirection::Backward),
    );
}

#[test]
fn startup_theme_mapping_preserves_the_os_observation() {
    assert_eq!(map_system_theme(Some(WinitTheme::Dark)), SystemTheme::Dark);
    assert_eq!(
        map_system_theme(Some(WinitTheme::Light)),
        SystemTheme::Light
    );
    assert_eq!(map_system_theme(None), SystemTheme::Light);
}

#[test]
fn terminal_paste_shortcuts_are_platform_correct_and_preserve_plain_control_v() {
    let key = |modifiers| KeyEvent {
        key: Key::Character("v".into()),
        modifiers,
        pressed: true,
    };

    assert!(is_paste_shortcut_for(
        &key(Modifiers::SUPER),
        true,
        ShortcutPlatform::MacOS,
    ));
    assert!(!is_paste_shortcut_for(
        &key(Modifiers::CONTROL),
        true,
        ShortcutPlatform::Other,
    ));
    assert!(is_paste_shortcut_for(
        &key(Modifiers::CONTROL | Modifiers::SHIFT),
        true,
        ShortcutPlatform::Other,
    ));
    assert_eq!(
        route_key_event(&key(Modifiers::CONTROL), true),
        InputRoute::TerminalPty,
    );
    assert!(is_paste_shortcut_for(
        &key(Modifiers::CONTROL),
        false,
        ShortcutPlatform::Other,
    ));
}

#[test]
fn ime_is_enabled_only_for_the_active_page_text_input() {
    assert!(ime_allowed_for_focus(
        zode_app_model::ShellPage::Conversation,
        Some(zode_app_ui::COMPOSER_ID),
        true,
    ));
    assert!(!ime_allowed_for_focus(
        zode_app_model::ShellPage::Conversation,
        Some(zode_app_ui::SEND_ID),
        true,
    ));
    assert!(ime_allowed_for_focus(
        zode_app_model::ShellPage::Terminal,
        Some(zode_app_ui::TERMINAL_ID),
        true,
    ));
    assert!(!ime_allowed_for_focus(
        zode_app_model::ShellPage::Settings,
        Some(zode_app_ui::THEME_SYSTEM_ID),
        true,
    ));
    assert!(ime_allowed_for_focus(
        zode_app_model::ShellPage::Settings,
        Some(zode_app_ui::SETTINGS_SEARCH_ID),
        true,
    ));
    assert!(!ime_allowed_for_focus(
        zode_app_model::ShellPage::Conversation,
        Some(zode_app_ui::COMPOSER_ID),
        false,
    ));
}
