//! Approval-policy checks for direct command and filesystem methods.

use crate::error::error;
use zode_app_server_protocol::rpc::{ErrorObject, POLICY_DENIED};
use zode_app_server_protocol::types::ApprovalPolicy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectKind {
    Command,
    FsWrite,
}

pub enum DirectGate {
    Allow,
    Deny(ErrorObject),
    Prompt,
}

/// Classifies a mutating direct method without collapsing prompt into allow.
pub fn direct_gate(policy: ApprovalPolicy, kind: DirectKind) -> DirectGate {
    match policy {
        ApprovalPolicy::ReadOnly => DirectGate::Deny(error(
            POLICY_DENIED,
            format!("approval policy denies direct {kind:?}"),
        )),
        ApprovalPolicy::Auto => DirectGate::Allow,
        ApprovalPolicy::Prompt => DirectGate::Prompt,
    }
}
