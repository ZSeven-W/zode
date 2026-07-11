use crate::policy::{direct_gate, DirectGate, DirectKind};
use zode_app_server_protocol::rpc::POLICY_DENIED;
use zode_app_server_protocol::types::ApprovalPolicy;

#[test]
fn read_only_denies_command_and_fs_write() {
    assert_eq!(
        match direct_gate(ApprovalPolicy::ReadOnly, DirectKind::Command) {
            DirectGate::Deny(error) => error.code,
            _ => panic!("read-only command must be denied"),
        },
        POLICY_DENIED
    );
    assert_eq!(
        match direct_gate(ApprovalPolicy::ReadOnly, DirectKind::FsWrite) {
            DirectGate::Deny(error) => error.code,
            _ => panic!("read-only fs write must be denied"),
        },
        POLICY_DENIED
    );
    assert!(matches!(
        direct_gate(ApprovalPolicy::Auto, DirectKind::Command),
        DirectGate::Allow
    ));
    assert!(matches!(
        direct_gate(ApprovalPolicy::Prompt, DirectKind::Command),
        DirectGate::Prompt
    ));
}
