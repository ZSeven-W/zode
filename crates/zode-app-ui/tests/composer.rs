use zode_app_model::AttachmentMetadata;
use zode_app_ui::{
    ComposerController, ComposerOutcome, ComposerSubmission, ImeEvent, Key, Modifiers,
    SandboxSelection,
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
    assert!(matches!(
        composer.paste_image("image/png", "aGVsbG8=", "reference.png"),
        ComposerOutcome::AttachmentsChanged(attachments)
            if attachments.len() == 1 && attachments[0].display_name == "reference.png"
    ));

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
fn submitting_clears_projected_attachments_without_dropping_payload() {
    let mut composer = ComposerController::fixture("describe this image");
    let metadata = AttachmentMetadata {
        id: "attachment-1".into(),
        path: None,
        display_name: "reference.png".into(),
        media_type: "image/png".into(),
        width: Some(640),
        height: Some(360),
        byte_len: 5,
    };
    assert_eq!(
        composer.paste_image_with_metadata("image/png", "aGVsbG8=", metadata.clone(),),
        ComposerOutcome::AttachmentsChanged(vec![metadata]),
    );

    let ComposerOutcome::Send(submission) = composer.key(Key::Enter, Modifiers::NONE) else {
        panic!("expected a send outcome");
    };

    assert!(composer.attachment_metadata().is_empty());
    assert!(matches!(
        &submission.content[1],
        UserContent::Image { data_base64, .. } if data_base64 == "aGVsbG8="
    ));
}

#[test]
fn same_named_attachments_receive_distinct_stable_ids() {
    let mut composer = ComposerController::fixture("");
    let first = composer.paste_image("image/png", "YQ==", "same.png");
    let second = composer.paste_image("image/png", "Yg==", "same.png");

    let ComposerOutcome::AttachmentsChanged(first) = first else {
        panic!("first image should project metadata");
    };
    let ComposerOutcome::AttachmentsChanged(second) = second else {
        panic!("second image should project metadata");
    };
    assert_ne!(first[0].id, second[1].id);
    assert_eq!(first[0].display_name, second[1].display_name);
}

#[test]
fn busy_composer_queues_and_exposes_stop() {
    let mut composer = ComposerController::fixture("change course");
    composer.set_busy(true);
    assert!(matches!(
        composer.key(Key::Enter, Modifiers::NONE),
        ComposerOutcome::Queue(_)
    ));
    assert_eq!(composer.stop(), ComposerOutcome::Stop);
}

#[test]
fn queue_edit_restores_the_unsubmitted_draft_and_attachments() {
    let mut composer = ComposerController::fixture("unsubmitted draft");
    assert!(matches!(
        composer.paste_image("image/png", "aGVsbG8=", "draft.png"),
        ComposerOutcome::AttachmentsChanged(_)
    ));
    let before = composer.attachment_metadata().to_vec();

    assert!(composer.begin_queue_edit("queued text"));
    assert!(composer.queue_editing());
    assert_eq!(composer.text(), "queued text");
    assert!(composer.attachment_metadata().is_empty());
    assert_eq!(
        composer.paste_image("image/png", "d29ybGQ=", "blocked.png"),
        ComposerOutcome::Ignored,
    );

    assert!(composer.finish_queue_edit());
    assert!(!composer.queue_editing());
    assert_eq!(composer.text(), "unsubmitted draft");
    assert_eq!(composer.attachment_metadata(), before);

    let ComposerOutcome::Send(submission) = composer.key(Key::Enter, Modifiers::NONE) else {
        panic!("restored draft should remain submit-ready");
    };
    assert!(matches!(
        submission.content.as_slice(),
        [UserContent::Text { text }, UserContent::Image { display_name, .. }]
            if text == "unsubmitted draft" && display_name == "draft.png"
    ));
}

#[test]
fn queue_edit_can_save_empty_text_for_an_attachment_only_message() {
    let mut composer = ComposerController::fixture("unsubmitted draft");
    composer.set_busy(true);
    assert!(composer.begin_queue_edit_for_message("queued caption", true));
    composer.set_text("");

    let ComposerOutcome::Queue(submission) = composer.key(Key::Enter, Modifiers::NONE) else {
        panic!("queue edit should emit an update even when only the queued attachments remain");
    };
    assert!(submission.content.is_empty());
}

#[test]
fn queue_edit_retargets_without_losing_the_original_draft_backup() {
    let mut composer = ComposerController::fixture("unsubmitted draft");
    assert!(matches!(
        composer.paste_image("image/png", "aGVsbG8=", "draft.png"),
        ComposerOutcome::AttachmentsChanged(_)
    ));
    let original_attachments = composer.attachment_metadata().to_vec();

    assert!(composer.begin_queue_edit_for_message("first queued message", false));
    composer.set_text("unsaved first edit");
    assert!(composer.begin_queue_edit_for_message("second queued message", true));
    assert_eq!(composer.text(), "second queued message");
    composer.set_text("");
    assert!(matches!(
        composer.key(Key::Enter, Modifiers::NONE),
        ComposerOutcome::Send(ComposerSubmission { content, .. }) if content.is_empty()
    ));

    assert!(composer.finish_queue_edit());
    assert_eq!(composer.text(), "unsubmitted draft");
    assert_eq!(composer.attachment_metadata(), original_attachments);
}

#[test]
fn picker_choices_are_explicit_outcomes() {
    let composer = ComposerController::fixture("");
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
