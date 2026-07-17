use std::sync::{Arc, Mutex};

use base64::{engine::general_purpose::STANDARD, Engine};
use jian_core::CursorHint;
use jian_widgets::Point2D;
use winit::window::CursorIcon;
use zode_app::{
    clipboard::{
        execute_clipboard_command, paste_from_clipboard, ClipboardImage, ClipboardService,
    },
    cursor::{cursor_hint_at, cursor_icon_for_hint},
    services::ServiceError,
};
use zode_app_model::AppCommand;
use zode_app_ui::{ComposerController, ComposerOutcome, Insets, Key, Modifiers, WorkspaceSnapshot};
use zode_node_protocol::UserContent;

struct FakeClipboard {
    text: Option<String>,
    image: Option<ClipboardImage>,
    writes: Mutex<Vec<String>>,
}

impl ClipboardService for FakeClipboard {
    fn read_text(&self) -> Result<Option<String>, ServiceError> {
        Ok(self.text.clone())
    }

    fn write_text(&self, text: &str) -> Result<(), ServiceError> {
        self.writes.lock().unwrap().push(text.into());
        Ok(())
    }

    fn read_image(&self) -> Result<Option<ClipboardImage>, ServiceError> {
        Ok(self.image.clone())
    }
}

fn accept_clipboard(_: Arc<dyn ClipboardService>) {}

#[test]
fn clipboard_service_is_object_safe_and_copy_commands_use_injected_service() {
    let clipboard = Arc::new(FakeClipboard {
        text: None,
        image: None,
        writes: Mutex::new(Vec::new()),
    });
    accept_clipboard(clipboard.clone());

    assert!(
        execute_clipboard_command(&AppCommand::CopyText("copied".into()), clipboard.as_ref(),)
            .unwrap()
    );
    assert_eq!(*clipboard.writes.lock().unwrap(), vec!["copied"]);
}

#[test]
fn injected_clipboard_pastes_text_and_rgba_image_into_composer() {
    let clipboard = FakeClipboard {
        text: Some("describe ".into()),
        image: Some(ClipboardImage {
            width: 1,
            height: 1,
            rgba8: vec![255, 0, 0, 255],
        }),
        writes: Mutex::new(Vec::new()),
    };
    let mut composer = ComposerController::fixture("");

    assert_eq!(paste_from_clipboard(&clipboard, &mut composer).unwrap(), 2,);
    let ComposerOutcome::Send(submission) = composer.key(Key::Enter, Modifiers::NONE) else {
        panic!("clipboard content should submit");
    };
    assert!(matches!(
        &submission.content[0],
        UserContent::Text { text } if text == "describe "
    ));
    let UserContent::Image {
        mime_type,
        data_base64,
        display_name,
    } = &submission.content[1]
    else {
        panic!("second clipboard item should be an image");
    };
    assert_eq!(mime_type, "image/png");
    assert_eq!(display_name, "clipboard.png");
    let png = STANDARD.decode(data_base64).unwrap();
    let decoded = image::load_from_memory(&png).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (1, 1));
}

#[test]
fn clipboard_paste_preserves_attachment_dimensions_and_byte_length() {
    let clipboard = FakeClipboard {
        text: None,
        image: Some(ClipboardImage {
            width: 2,
            height: 3,
            rgba8: vec![255; 2 * 3 * 4],
        }),
        writes: Mutex::new(Vec::new()),
    };
    let mut composer = ComposerController::fixture("");

    assert_eq!(paste_from_clipboard(&clipboard, &mut composer).unwrap(), 1);
    let [attachment] = composer.attachment_metadata() else {
        panic!("one attachment should be projected");
    };
    assert_eq!((attachment.width, attachment.height), (Some(2), Some(3)));
    assert!(attachment.byte_len > 0);
    assert_eq!(attachment.display_name, "clipboard.png");
}

#[test]
fn invalid_rgba_clipboard_image_is_rejected() {
    let clipboard = FakeClipboard {
        text: None,
        image: Some(ClipboardImage {
            width: 2,
            height: 2,
            rgba8: vec![0; 3],
        }),
        writes: Mutex::new(Vec::new()),
    };

    let mut composer = ComposerController::fixture("before");
    assert!(paste_from_clipboard(&clipboard, &mut composer).is_err());
    assert_eq!(composer.text(), "before");
}

#[test]
fn neutral_cursor_hints_map_purely_to_casement_icons() {
    for (hint, icon) in [
        (CursorHint::Default, CursorIcon::Default),
        (CursorHint::Pointer, CursorIcon::Pointer),
        (CursorHint::Text, CursorIcon::Text),
        (CursorHint::Grab, CursorIcon::Grab),
        (CursorHint::Grabbing, CursorIcon::Grabbing),
        (CursorHint::ResizeEw, CursorIcon::EwResize),
        (CursorHint::ResizeNs, CursorIcon::NsResize),
        (CursorHint::ResizeNwse, CursorIcon::NwseResize),
        (CursorHint::ResizeNesw, CursorIcon::NeswResize),
        (CursorHint::Rotate, CursorIcon::Alias),
    ] {
        assert_eq!(cursor_icon_for_hint(hint), icon);
    }
}

#[test]
fn cursor_hint_comes_from_the_shared_snapshot_hit_target() {
    let snapshot =
        WorkspaceSnapshot::build(&zode_app_model::demo_state(), 1221.0, 992.0, Insets::ZERO);
    let composer = snapshot
        .nodes
        .iter()
        .find(|node| node.cursor == CursorHint::Text)
        .unwrap();
    let point = Point2D::new(
        composer.rect.origin.x + composer.rect.size.x / 2.0,
        composer.rect.origin.y + composer.rect.size.y / 2.0,
    );

    assert_eq!(cursor_hint_at(&snapshot, point), CursorHint::Text);
    assert_eq!(
        cursor_icon_for_hint(cursor_hint_at(&snapshot, point)),
        CursorIcon::Text
    );
}

#[test]
fn terminal_copy_uses_the_local_clipboard_command_gateway() {
    let app = include_str!("../src/app.rs");
    let terminal = include_str!("../src/app/terminal.rs");
    let interaction = include_str!("../src/app/interaction.rs");
    let copy_branch = terminal
        .split("TerminalPanelController::is_copy_shortcut")
        .nth(1)
        .expect("terminal copy shortcut branch");

    assert!(copy_branch.contains("self.enqueue_command(command)"));
    assert!(!app.contains("pending_commands.push_back"));
    assert!(!terminal.contains("pending_commands.push_back"));
    assert!(!interaction.contains("pending_commands.push_back"));
    assert!(interaction.contains("prepare_dispatch"));
    assert!(app.contains("CommandBridge::spawn"));
    assert!(interaction.contains("matches!(command, AppCommand::CopyText(_))"));
}

#[test]
fn native_clipboard_build_enables_wayland_and_reports_initialization_failure() {
    let workspace_manifest = include_str!("../../../Cargo.toml");
    let app = include_str!("../src/app.rs");

    assert!(workspace_manifest.contains("features = [\"wayland-data-control\"]"));
    assert!(app.contains("native clipboard is unavailable"));
}
