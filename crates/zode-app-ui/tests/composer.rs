use zode_app_ui::{
    ComposerController, ComposerOutcome, ImeEvent, Key, Modifiers, SandboxSelection,
};
use zode_node_protocol::{SandboxMode, UserContent};

#[test]
fn enter_sends_but_shift_enter_inserts_newline() {
    let mut composer = ComposerController::fixture("hello");
    assert_eq!(
        composer.key(Key::Enter, Modifiers::NONE),
        ComposerOutcome::Send("hello".into()),
    );
    assert_eq!(composer.text(), "");

    let mut composer = ComposerController::fixture("hello");
    assert_eq!(
        composer.key(Key::Enter, Modifiers::SHIFT),
        ComposerOutcome::Edited,
    );
    assert_eq!(composer.text(), "hello\n");
}

#[test]
fn whitespace_only_input_does_not_send() {
    let mut composer = ComposerController::fixture("  \n  ");
    assert_eq!(
        composer.key(Key::Enter, Modifiers::NONE),
        ComposerOutcome::Ignored,
    );
    assert_eq!(composer.text(), "  \n  ");
}

#[test]
fn ime_commit_is_applied_once_and_end_only_clears_preedit() {
    let mut composer = ComposerController::fixture("");
    assert_eq!(composer.ime(ImeEvent::Start), ComposerOutcome::Edited);
    assert_eq!(
        composer.ime(ImeEvent::Update {
            text: "中".into(),
            cursor: Some("中".len()),
        }),
        ComposerOutcome::Edited,
    );
    assert_eq!(composer.text(), "");
    assert_eq!(composer.composition_text(), Some("中"));

    assert_eq!(
        composer.ime(ImeEvent::Commit("中文".into())),
        ComposerOutcome::Edited,
    );
    assert_eq!(composer.ime(ImeEvent::End), ComposerOutcome::Edited);
    assert_eq!(composer.text(), "中文");
    assert_eq!(composer.composition_text(), None);
}

#[test]
fn paste_preserves_text_and_image_in_submission() {
    let mut composer = ComposerController::fixture("describe");
    assert_eq!(composer.paste_text(" this"), ComposerOutcome::Edited);
    assert_eq!(
        composer.paste_image("image/png", "aGVsbG8=", "reference.png"),
        ComposerOutcome::Edited,
    );

    let ComposerOutcome::Send(submission) = composer.key(Key::Enter, Modifiers::NONE) else {
        panic!("expected a send outcome");
    };
    assert_eq!(
        submission.content,
        vec![
            UserContent::Text {
                text: "describe this".into(),
            },
            UserContent::Image {
                mime_type: "image/png".into(),
                data_base64: "aGVsbG8=".into(),
                display_name: "reference.png".into(),
            },
        ],
    );
}

#[test]
fn busy_composer_steers_and_exposes_stop() {
    let mut composer = ComposerController::fixture("change course");
    composer.set_busy(true);
    assert!(matches!(
        composer.key(Key::Enter, Modifiers::NONE),
        ComposerOutcome::Steer(_)
    ));
    assert_eq!(composer.stop(), ComposerOutcome::Stop);
}

#[test]
fn picker_choices_are_explicit_outcomes() {
    let mut composer = ComposerController::fixture("");
    assert_eq!(
        composer.select_model("gpt-5.2"),
        ComposerOutcome::SetModel("gpt-5.2".into()),
    );
    assert_eq!(
        composer.select_effort("high"),
        ComposerOutcome::SetEffort("high".into()),
    );
    assert_eq!(
        composer.select_sandbox(SandboxSelection {
            mode: SandboxMode::WorkspaceWrite,
            network: false,
        }),
        ComposerOutcome::SetSandbox(SandboxSelection {
            mode: SandboxMode::WorkspaceWrite,
            network: false,
        }),
    );
}
