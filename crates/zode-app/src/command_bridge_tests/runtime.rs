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
