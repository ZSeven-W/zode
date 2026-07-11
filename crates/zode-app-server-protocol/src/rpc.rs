use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The literal "2.0" protocol tag. Serializes to "2.0", refuses anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct V2;

impl Serialize for V2 {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str("2.0")
    }
}

impl<'de> Deserialize<'de> for V2 {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = String::deserialize(d)?;
        if v == "2.0" {
            Ok(V2)
        } else {
            Err(serde::de::Error::custom("jsonrpc must be \"2.0\""))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
    #[serde(with = "unit_null")]
    Null,
}

mod unit_null {
    use serde::Deserialize;

    pub fn serialize<S: serde::Serializer>(s: S) -> Result<S::Ok, S::Error> {
        s.serialize_none()
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<(), D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        if v.is_null() {
            Ok(())
        } else {
            Err(serde::de::Error::custom("expected null"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcRequest {
    pub jsonrpc: V2,
    pub id: RequestId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: RequestId, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: V2,
            id,
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcNotification {
    pub jsonrpc: V2,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: V2,
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcResponse {
    pub jsonrpc: V2,
    pub id: RequestId,
    pub result: Value,
}

impl JsonRpcResponse {
    pub fn new(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: V2,
            id,
            result,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorObject {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcError {
    pub jsonrpc: V2,
    pub id: RequestId,
    pub error: ErrorObject,
}

impl JsonRpcError {
    pub fn new(id: RequestId, error: ErrorObject) -> Self {
        Self {
            jsonrpc: V2,
            id,
            error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Error(JsonRpcError),
    Response(JsonRpcResponse),
    Request(JsonRpcRequest),
    Notification(JsonRpcNotification),
}

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
pub const SERVER_OVERLOADED: i64 = -32001;
pub const NOT_INITIALIZED: i64 = -32002;
pub const ALREADY_INITIALIZED: i64 = -32003;
pub const POLICY_DENIED: i64 = -32010;
pub const TURN_ACTIVE: i64 = -32011;
pub const APPROVAL_TIMEOUT: i64 = -32012;
pub const UNAUTHORIZED: i64 = -32013;
