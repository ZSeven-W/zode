//! Approval-policy checks for direct command and filesystem methods.

use crate::error::error;
use zode_app_server_protocol::rpc::{ErrorObject, POLICY_DENIED};
use zode_app_server_protocol::types::ApprovalPolicy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectKind {
    Command,
    FsWrite,
}

/// Read-only denies mutating direct methods with `POLICY_DENIED`; auto allows.
pub fn check_direct(policy: ApprovalPolicy, kind: DirectKind) -> Result<(), ErrorObject> {
    match policy {
        ApprovalPolicy::ReadOnly => Err(error(
            POLICY_DENIED,
            format!("approval policy denies direct {kind:?}"),
        )),
        ApprovalPolicy::Auto | ApprovalPolicy::Prompt => Ok(()),
    }
}
