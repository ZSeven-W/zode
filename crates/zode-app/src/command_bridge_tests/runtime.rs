use zode_app_model::AppCommand;
use zode_node_protocol::{AgentCommandKind, ApprovalMode, SandboxMode, TurnId};

use super::{default_runtime_options, fixture, prepare_dispatch};

#[test]
fn active_turn_rejects_runtime_changes_at_the_command_boundary() {
    let mut state = fixture();
    let session = state.current_session.clone().expect("fixture session");
    state.active_turns.insert(session, TurnId::new());
    state.composer_defaults = Some(default_runtime_options());

    for command in [
        AppCommand::SetModel("model-b".into()),
        AppCommand::SetEffort("high".into()),
        AppCommand::SetSandbox {
            mode: SandboxMode::Off,
            network: false,
        },
        AppCommand::ResetComposerRuntime,
    ] {
        let error = prepare_dispatch(&mut state, command)
            .expect_err("runtime changes must be rejected while a turn is active");
        assert!(error.contains("while the task is running"));
    }
}

#[test]
fn permission_preset_dispatch_is_atomic_and_rejected_during_an_active_turn() {
    let mut state = fixture();
    let session = state.current_session.clone().unwrap();
    let command = AppCommand::SetPermissionPreset {
        approval_mode: ApprovalMode::Auto,
        sandbox_mode: SandboxMode::WorkspaceWrite,
        network: true,
    };
    let dispatch = prepare_dispatch(&mut state, command.clone())
        .unwrap()
        .unwrap();
    assert_eq!(dispatch.commands.len(), 1);
    assert!(matches!(
        dispatch.commands[0].kind,
        AgentCommandKind::SetPermissionPreset {
            approval_mode: ApprovalMode::Auto,
            sandbox_mode: SandboxMode::WorkspaceWrite,
            network: true,
        }
    ));

    state.active_turns.insert(session, TurnId::new());
    assert!(prepare_dispatch(&mut state, command).is_err());
}

#[test]
fn new_task_model_selection_is_local_until_session_creation() {
    let mut state = fixture();
    state.current_session = None;
    state.composer.available_models = vec!["model-a".into(), "model-b".into()];
    state.composer.model = Some("model-a".into());

    let dispatch = prepare_dispatch(&mut state, AppCommand::SetModel("model-b".into())).unwrap();

    assert!(dispatch.is_none());
    assert_eq!(state.composer.model.as_deref(), Some("model-b"));
    assert!(prepare_dispatch(&mut state, AppCommand::SetModel("unknown".into())).is_err());
}

/// The mid-conversation model-switch warning (`model-switch-notice.rs`).
mod model_switch_notice {
    use zode_app_model::{AppCommand, TranscriptItem, ZodeAppState};

    use super::super::{fixture, prepare_dispatch};

    const CODE: &str = "session.model_switched";

    fn notices(state: &ZodeAppState) -> Vec<String> {
        let session = state.current_session.clone().unwrap();
        state.transcripts[&session]
            .items
            .iter()
            .filter_map(|item| match item {
                TranscriptItem::Status { code, message } if code == CODE => Some(message.clone()),
                _ => None,
            })
            .collect()
    }

    fn started_conversation() -> ZodeAppState {
        let mut state = fixture();
        let session = state.current_session.clone().unwrap();
        state.composer.model = Some("model-a".into());
        state
            .transcripts
            .get_mut(&session)
            .unwrap()
            .items
            .push(TranscriptItem::user_text("先看看这个仓库"));
        state
    }

    #[test]
    fn switching_mid_conversation_warns_once_and_names_the_new_model() {
        let mut state = started_conversation();

        prepare_dispatch(&mut state, AppCommand::SetModel("model-b".into())).unwrap();

        let notices = notices(&state);
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].contains("model-b"),
            "the notice names the model being switched to: {}",
            notices[0]
        );
    }

    #[test]
    fn flipping_between_models_rewrites_the_same_notice() {
        let mut state = started_conversation();

        prepare_dispatch(&mut state, AppCommand::SetModel("model-b".into())).unwrap();
        state.composer.model = Some("model-b".into());
        prepare_dispatch(&mut state, AppCommand::SetModel("model-a".into())).unwrap();

        let notices = notices(&state);
        assert_eq!(notices.len(), 1, "the warning is rewritten, not stacked");
        assert!(notices[0].contains("model-a"));
    }

    #[test]
    fn selecting_the_same_model_again_says_nothing() {
        let mut state = started_conversation();

        prepare_dispatch(&mut state, AppCommand::SetModel("model-a".into())).unwrap();

        assert!(notices(&state).is_empty());
    }

    #[test]
    fn a_task_with_no_conversation_yet_is_not_a_switch() {
        let mut state = fixture();
        state.composer.model = Some("model-a".into());

        prepare_dispatch(&mut state, AppCommand::SetModel("model-b".into())).unwrap();

        assert!(
            notices(&state).is_empty(),
            "picking a model before the first message is a preference, not a switch"
        );
    }
}
