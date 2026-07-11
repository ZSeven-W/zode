use crate::policy::{check_direct, DirectKind};
use zode_app_server_protocol::rpc::POLICY_DENIED;
use zode_app_server_protocol::types::ApprovalPolicy;

#[test]
fn read_only_denies_command_and_fs_write() {
    assert_eq!(
        check_direct(ApprovalPolicy::ReadOnly, DirectKind::Command)
            .unwrap_err()
            .code,
        POLICY_DENIED
    );
    assert_eq!(
        check_direct(ApprovalPolicy::ReadOnly, DirectKind::FsWrite)
            .unwrap_err()
            .code,
        POLICY_DENIED
    );
    assert!(check_direct(ApprovalPolicy::Auto, DirectKind::Command).is_ok());
}
