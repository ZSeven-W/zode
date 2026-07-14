use serde::{Deserialize, Deserializer, Serialize};

/// Version advertised by the bridge authentication hello. A client that does
/// not see this exact version can keep using legacy browser RPCs, but must not
/// send task-channel requests.
pub const TASK_PROTOCOL_VERSION: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskChannel {
    #[serde(rename = "tasks")]
    Tasks,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskClientFrame {
    pub channel: TaskChannel,
    #[serde(flatten)]
    pub body: TaskClientBody,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TaskClientBody {
    Request {
        id: String,
        method: String,
        #[serde(default = "empty_object")]
        params: serde_json::Value,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskServerFrame {
    pub channel: TaskChannel,
    #[serde(flatten)]
    pub body: TaskServerBody,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TaskServerBody {
    Response {
        id: String,
        result: serde_json::Value,
    },
    Error {
        id: String,
        code: String,
        message: String,
    },
    Event {
        event: String,
        params: serde_json::Value,
    },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum StrictTaskClientFrame {
    Request(StrictTaskClientRequestFrame),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictTaskClientRequestFrame {
    channel: TaskChannel,
    id: String,
    method: String,
    #[serde(default = "empty_object")]
    params: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum StrictTaskClientBody {
    Request(StrictTaskClientRequestBody),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictTaskClientRequestBody {
    id: String,
    method: String,
    #[serde(default = "empty_object")]
    params: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum StrictTaskServerFrame {
    Response(StrictTaskServerResponseFrame),
    Error(StrictTaskServerErrorFrame),
    Event(StrictTaskServerEventFrame),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictTaskServerResponseFrame {
    channel: TaskChannel,
    id: String,
    result: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictTaskServerErrorFrame {
    channel: TaskChannel,
    id: String,
    code: String,
    message: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictTaskServerEventFrame {
    channel: TaskChannel,
    event: String,
    params: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum StrictTaskServerBody {
    Response(StrictTaskServerResponseBody),
    Error(StrictTaskServerErrorBody),
    Event(StrictTaskServerEventBody),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictTaskServerResponseBody {
    id: String,
    result: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictTaskServerErrorBody {
    id: String,
    code: String,
    message: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictTaskServerEventBody {
    event: String,
    params: serde_json::Value,
}

impl<'de> Deserialize<'de> for TaskClientFrame {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictTaskClientFrame::deserialize(deserializer).map(Into::into)
    }
}

impl<'de> Deserialize<'de> for TaskClientBody {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictTaskClientBody::deserialize(deserializer).map(Into::into)
    }
}

impl<'de> Deserialize<'de> for TaskServerFrame {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictTaskServerFrame::deserialize(deserializer).map(Into::into)
    }
}

impl<'de> Deserialize<'de> for TaskServerBody {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictTaskServerBody::deserialize(deserializer).map(Into::into)
    }
}

impl From<StrictTaskClientFrame> for TaskClientFrame {
    fn from(value: StrictTaskClientFrame) -> Self {
        match value {
            StrictTaskClientFrame::Request(frame) => Self {
                channel: frame.channel,
                body: TaskClientBody::Request {
                    id: frame.id,
                    method: frame.method,
                    params: frame.params,
                },
            },
        }
    }
}

impl From<StrictTaskClientBody> for TaskClientBody {
    fn from(value: StrictTaskClientBody) -> Self {
        match value {
            StrictTaskClientBody::Request(body) => Self::Request {
                id: body.id,
                method: body.method,
                params: body.params,
            },
        }
    }
}

impl From<StrictTaskServerFrame> for TaskServerFrame {
    fn from(value: StrictTaskServerFrame) -> Self {
        match value {
            StrictTaskServerFrame::Response(frame) => Self {
                channel: frame.channel,
                body: TaskServerBody::Response {
                    id: frame.id,
                    result: frame.result,
                },
            },
            StrictTaskServerFrame::Error(frame) => Self {
                channel: frame.channel,
                body: TaskServerBody::Error {
                    id: frame.id,
                    code: frame.code,
                    message: frame.message,
                },
            },
            StrictTaskServerFrame::Event(frame) => Self {
                channel: frame.channel,
                body: TaskServerBody::Event {
                    event: frame.event,
                    params: frame.params,
                },
            },
        }
    }
}

impl From<StrictTaskServerBody> for TaskServerBody {
    fn from(value: StrictTaskServerBody) -> Self {
        match value {
            StrictTaskServerBody::Response(body) => Self::Response {
                id: body.id,
                result: body.result,
            },
            StrictTaskServerBody::Error(body) => Self::Error {
                id: body.id,
                code: body.code,
                message: body.message,
            },
            StrictTaskServerBody::Event(body) => Self::Event {
                event: body.event,
                params: body.params,
            },
        }
    }
}

impl TaskClientFrame {
    pub fn request(
        id: impl Into<String>,
        method: impl Into<String>,
        params: serde_json::Value,
    ) -> Self {
        Self {
            channel: TaskChannel::Tasks,
            body: TaskClientBody::Request {
                id: id.into(),
                method: method.into(),
                params,
            },
        }
    }
}

impl TaskServerFrame {
    pub fn response(id: impl Into<String>, result: serde_json::Value) -> Self {
        Self {
            channel: TaskChannel::Tasks,
            body: TaskServerBody::Response {
                id: id.into(),
                result,
            },
        }
    }

    pub fn error(
        id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            channel: TaskChannel::Tasks,
            body: TaskServerBody::Error {
                id: id.into(),
                code: code.into(),
                message: message.into(),
            },
        }
    }

    pub fn event(event: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            channel: TaskChannel::Tasks,
            body: TaskServerBody::Event {
                event: event.into(),
                params,
            },
        }
    }
}

fn empty_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_with_tasks_channel() {
        let frame = TaskClientFrame::request(
            "ext-7",
            "turn/start",
            serde_json::json!({"taskId":"s1","input":"inspect"}),
        );
        let value = serde_json::to_value(&frame).unwrap();

        assert_eq!(value["channel"], "tasks");
        assert_eq!(value["kind"], "request");
        assert_eq!(value["id"], "ext-7");
        assert_eq!(
            serde_json::from_value::<TaskClientFrame>(value).unwrap(),
            frame
        );
    }

    #[test]
    fn server_frames_keep_response_and_event_distinct() {
        let response = TaskServerFrame::response("ext-7", serde_json::json!({"ok":true}));
        let event = TaskServerFrame::event(
            "message/delta",
            serde_json::json!({"taskId":"s1","turnId":"1","delta":"hi"}),
        );

        let response_value = serde_json::to_value(&response).unwrap();
        let event_value = serde_json::to_value(&event).unwrap();

        assert_eq!(
            response_value,
            serde_json::json!({
                "channel": "tasks",
                "kind": "response",
                "id": "ext-7",
                "result": {"ok": true}
            })
        );
        assert_eq!(
            event_value,
            serde_json::json!({
                "channel": "tasks",
                "kind": "event",
                "event": "message/delta",
                "params": {"taskId":"s1","turnId":"1","delta":"hi"}
            })
        );
        assert_eq!(
            serde_json::from_value::<TaskServerFrame>(response_value).unwrap(),
            response
        );
        assert_eq!(
            serde_json::from_value::<TaskServerFrame>(event_value).unwrap(),
            event
        );
    }

    #[test]
    fn a_non_tasks_channel_is_rejected() {
        let err = serde_json::from_value::<TaskClientFrame>(serde_json::json!({
            "channel":"browser",
            "kind":"request",
            "id":"1",
            "method":"snapshot/read",
            "params":{}
        }))
        .unwrap_err();

        assert!(err.to_string().contains("tasks"));
    }

    #[test]
    fn error_constructor_has_the_strict_wire_shape() {
        let frame = TaskServerFrame::error("ext-7", "invalid_frame", "bad request");

        assert_eq!(
            serde_json::to_value(frame).unwrap(),
            serde_json::json!({
                "channel": "tasks",
                "kind": "error",
                "id": "ext-7",
                "code": "invalid_frame",
                "message": "bad request"
            })
        );
    }

    #[test]
    fn missing_request_params_default_to_empty_object() {
        let frame = serde_json::from_value::<TaskClientFrame>(serde_json::json!({
            "channel": "tasks",
            "kind": "request",
            "id": "ext-7",
            "method": "snapshot/read"
        }))
        .unwrap();

        assert_eq!(
            frame,
            TaskClientFrame::request("ext-7", "snapshot/read", serde_json::json!({}))
        );
    }

    fn assert_client_rejected_with_field(value: serde_json::Value, field: &str) {
        let err = serde_json::from_value::<TaskClientFrame>(value).unwrap_err();

        assert!(
            err.to_string().contains(field),
            "expected error to mention {field:?}, got {err}"
        );
    }

    #[test]
    fn request_rejects_misspelled_params() {
        assert_client_rejected_with_field(
            serde_json::json!({
                "channel": "tasks",
                "kind": "request",
                "id": "ext-7",
                "method": "snapshot/read",
                "param": {}
            }),
            "param",
        );
    }

    #[test]
    fn request_rejects_fields_from_other_kinds_and_unknown_fields() {
        for (value, field) in [
            (
                serde_json::json!({
                    "channel": "tasks",
                    "kind": "request",
                    "id": "ext-7",
                    "method": "snapshot/read",
                    "params": {},
                    "result": {"ok": true}
                }),
                "result",
            ),
            (
                serde_json::json!({
                    "channel": "tasks",
                    "kind": "request",
                    "id": "ext-7",
                    "method": "snapshot/read",
                    "params": {},
                    "unexpected": true
                }),
                "unexpected",
            ),
        ] {
            assert_client_rejected_with_field(value, field);
        }
    }

    fn assert_server_rejected_with_field(value: serde_json::Value, field: &str) {
        let err = serde_json::from_value::<TaskServerFrame>(value).unwrap_err();

        assert!(
            err.to_string().contains(field),
            "expected error to mention {field:?}, got {err}"
        );
    }

    #[test]
    fn server_bodies_reject_cross_kind_and_unknown_fields() {
        for (value, field) in [
            (
                serde_json::json!({
                    "channel": "tasks",
                    "kind": "response",
                    "id": "ext-7",
                    "result": {"ok": true},
                    "event": "message/delta"
                }),
                "event",
            ),
            (
                serde_json::json!({
                    "channel": "tasks",
                    "kind": "error",
                    "id": "ext-7",
                    "code": "invalid_frame",
                    "message": "bad request",
                    "result": null
                }),
                "result",
            ),
            (
                serde_json::json!({
                    "channel": "tasks",
                    "kind": "event",
                    "event": "message/delta",
                    "params": {"delta": "hi"},
                    "id": "ext-7"
                }),
                "id",
            ),
            (
                serde_json::json!({
                    "channel": "tasks",
                    "kind": "event",
                    "event": "message/delta",
                    "params": {"delta": "hi"},
                    "unexpected": true
                }),
                "unexpected",
            ),
        ] {
            assert_server_rejected_with_field(value, field);
        }
    }

    #[test]
    fn client_body_round_trips_and_defaults_missing_params() {
        let body = TaskClientBody::Request {
            id: "ext-7".to_string(),
            method: "turn/start".to_string(),
            params: serde_json::json!({"input": "inspect"}),
        };
        let value = serde_json::to_value(&body).unwrap();

        assert_eq!(value["kind"], "request");
        assert_eq!(
            serde_json::from_value::<TaskClientBody>(value).unwrap(),
            body
        );
        assert_eq!(
            serde_json::from_value::<TaskClientBody>(serde_json::json!({
                "kind": "request",
                "id": "ext-8",
                "method": "snapshot/read"
            }))
            .unwrap(),
            TaskClientBody::Request {
                id: "ext-8".to_string(),
                method: "snapshot/read".to_string(),
                params: serde_json::json!({}),
            }
        );
    }

    #[test]
    fn client_body_rejects_misspelled_and_unknown_fields() {
        for (value, field) in [
            (
                serde_json::json!({
                    "kind": "request",
                    "id": "ext-7",
                    "method": "snapshot/read",
                    "param": {}
                }),
                "param",
            ),
            (
                serde_json::json!({
                    "kind": "request",
                    "id": "ext-7",
                    "method": "snapshot/read",
                    "params": {},
                    "unexpected": true
                }),
                "unexpected",
            ),
        ] {
            let err = serde_json::from_value::<TaskClientBody>(value).unwrap_err();
            assert!(
                err.to_string().contains(field),
                "expected error to mention {field:?}, got {err}"
            );
        }
    }

    #[test]
    fn server_bodies_round_trip_with_distinct_shapes() {
        let cases = [
            TaskServerBody::Response {
                id: "ext-7".to_string(),
                result: serde_json::json!({"ok": true}),
            },
            TaskServerBody::Error {
                id: "ext-8".to_string(),
                code: "invalid_frame".to_string(),
                message: "bad request".to_string(),
            },
            TaskServerBody::Event {
                event: "message/delta".to_string(),
                params: serde_json::json!({"delta": "hi"}),
            },
        ];

        for body in cases {
            let value = serde_json::to_value(&body).unwrap();
            assert_eq!(
                serde_json::from_value::<TaskServerBody>(value).unwrap(),
                body
            );
        }
    }

    #[test]
    fn server_bodies_directly_reject_cross_kind_and_unknown_fields() {
        for (value, field) in [
            (
                serde_json::json!({
                    "kind": "response",
                    "id": "ext-7",
                    "result": {"ok": true},
                    "event": "message/delta"
                }),
                "event",
            ),
            (
                serde_json::json!({
                    "kind": "error",
                    "id": "ext-7",
                    "code": "invalid_frame",
                    "message": "bad request",
                    "result": null
                }),
                "result",
            ),
            (
                serde_json::json!({
                    "kind": "event",
                    "event": "message/delta",
                    "params": {"delta": "hi"},
                    "id": "ext-7"
                }),
                "id",
            ),
            (
                serde_json::json!({
                    "kind": "event",
                    "event": "message/delta",
                    "params": {"delta": "hi"},
                    "unexpected": true
                }),
                "unexpected",
            ),
        ] {
            let err = serde_json::from_value::<TaskServerBody>(value).unwrap_err();
            assert!(
                err.to_string().contains(field),
                "expected error to mention {field:?}, got {err}"
            );
        }
    }
}
