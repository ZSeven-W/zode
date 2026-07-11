//! Constructors and wire types for server-to-client JSON-RPC requests.

use crate::rpc::{JsonRpcRequest, RequestId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalKind {
    Tool,
    Command,
    FsWrite,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequestParams {
    pub approval_id: String,
    pub kind: ApprovalKind,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalDecision {
    Allow,
    AllowAlways,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalResponseResult {
    pub decision: ApprovalDecision,
}

pub fn approval_request(id: RequestId, params: &ApprovalRequestParams) -> JsonRpcRequest {
    JsonRpcRequest::new(
        id,
        "approval/request",
        Some(serde_json::to_value(params).expect("approval request params must serialize")),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        approval_request, ApprovalDecision, ApprovalKind, ApprovalRequestParams,
        ApprovalResponseResult,
    };
    use crate::rpc::RequestId;
    use serde_json::json;

    #[test]
    fn approval_decision_wire_names_are_camel_case() {
        assert_eq!(
            serde_json::to_value(ApprovalDecision::AllowAlways).unwrap(),
            json!("allowAlways")
        );
        assert_eq!(
            serde_json::to_value(ApprovalResponseResult {
                decision: ApprovalDecision::Deny,
            })
            .unwrap(),
            json!({"decision": "deny"})
        );
    }

    #[test]
    fn approval_request_builds_json_rpc_frame() {
        let request = approval_request(
            RequestId::String("approval-rpc-1".to_string()),
            &ApprovalRequestParams {
                approval_id: "approval-1".to_string(),
                kind: ApprovalKind::FsWrite,
                summary: "Write configuration".to_string(),
                thread_id: Some("thread-1".to_string()),
                turn_id: None,
                tool: None,
                input: Some(json!({"path": "/tmp/config"})),
            },
        );

        assert_eq!(request.id, RequestId::String("approval-rpc-1".to_string()));
        assert_eq!(request.method, "approval/request");
        assert_eq!(
            request.params,
            Some(json!({
                "approvalId": "approval-1",
                "kind": "fsWrite",
                "summary": "Write configuration",
                "threadId": "thread-1",
                "input": {"path": "/tmp/config"}
            }))
        );
    }
}
