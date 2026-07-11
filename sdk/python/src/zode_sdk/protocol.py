from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Any, TypedDict, Union

# Every JSON-RPC frame on the wire carries this exact version string. The zode
# app-server is strict: outgoing frames must include it and incoming frames
# without it are rejected by ``classify_incoming_frame``.
JSONRPC_VERSION = "2.0"

RequestId = Union[int, str]


class ProtocolMethod(str, Enum):
    INITIALIZE = "initialize"
    THREAD_START = "thread/start"
    THREAD_RESUME = "thread/resume"
    THREAD_LIST = "thread/list"
    THREAD_READ = "thread/read"
    THREAD_DELETE = "thread/delete"
    THREAD_NAME_SET = "thread/name/set"
    TURN_START = "turn/start"
    TURN_INTERRUPT = "turn/interrupt"
    FS_READ_FILE = "fs/readFile"
    FS_WRITE_FILE = "fs/writeFile"
    FS_CREATE_DIRECTORY = "fs/createDirectory"
    FS_GET_METADATA = "fs/getMetadata"
    FS_READ_DIRECTORY = "fs/readDirectory"
    FS_REMOVE = "fs/remove"
    FS_COPY = "fs/copy"
    COMMAND_EXEC = "command/exec"
    MODEL_LIST = "model/list"
    MODEL_SET = "model/set"
    CONFIG_READ = "config/read"
    CONFIG_LIST = "config/list"
    CONFIG_WRITE = "config/write"
    SKILLS_LIST = "skills/list"
    SKILLS_READ = "skills/read"
    HOOKS_LIST = "hooks/list"
    MCP_SERVER_STATUS_LIST = "mcpServerStatus/list"
    PLUGIN_LIST = "plugin/list"


def method_wire_name(method: "str | ProtocolMethod") -> str:
    """Return the string wire name for a method, accepting an enum or a str."""
    return method.value if isinstance(method, ProtocolMethod) else str(method)


def request_frame(
    request_id: RequestId,
    method: "str | ProtocolMethod",
    params: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Serialize a client->server request, always stamping ``jsonrpc``."""
    frame: dict[str, Any] = {
        "jsonrpc": JSONRPC_VERSION,
        "id": request_id,
        "method": method_wire_name(method),
    }
    if params is not None:
        frame["params"] = params
    return frame


def notification_frame(
    method: "str | ProtocolMethod", params: dict[str, Any] | None = None
) -> dict[str, Any]:
    """Serialize a client->server notification (no ``id``)."""
    frame: dict[str, Any] = {"jsonrpc": JSONRPC_VERSION, "method": method_wire_name(method)}
    if params is not None:
        frame["params"] = params
    return frame


def initialize_params(
    name: str, version: str, approval_policy: str | None = None
) -> dict[str, Any]:
    """Build ``initialize`` params. ``approval_policy=None`` omits the field so
    the server applies its own default (``readOnly``)."""
    params: dict[str, Any] = {"clientInfo": {"name": name, "version": version}}
    if approval_policy is not None:
        params["approvalPolicy"] = approval_policy
    return params


@dataclass(frozen=True)
class ClassifiedFrame:
    """An incoming frame tagged with its JSON-RPC kind.

    ``kind`` is one of ``"response"``, ``"error"``, ``"notification"``,
    ``"serverRequest"``. ``frame`` is the raw decoded dict.
    """

    kind: str
    frame: dict[str, Any]


def classify_incoming_frame(value: Any) -> ClassifiedFrame:
    """Classify a decoded incoming frame, rejecting anything that is not a
    strict JSON-RPC 2.0 object. Raises ``ValueError`` for frames missing the
    ``jsonrpc`` marker or otherwise malformed."""
    if not isinstance(value, dict) or value.get("jsonrpc") != JSONRPC_VERSION:
        raise ValueError("invalid JSON-RPC 2.0 frame")
    if "method" in value:
        if not isinstance(value["method"], str):
            raise ValueError("invalid JSON-RPC method")
        kind = "serverRequest" if "id" in value else "notification"
        return ClassifiedFrame(kind, value)
    if "id" not in value:
        raise ValueError("invalid JSON-RPC response")
    if "error" in value:
        return ClassifiedFrame("error", value)
    if "result" in value:
        return ClassifiedFrame("response", value)
    raise ValueError("invalid JSON-RPC response")


@dataclass
class JsonRpcRequest:
    id: RequestId
    method: str
    params: dict[str, Any] | None = None


class RpcErrorObject(TypedDict, total=False):
    code: int
    message: str
    data: Any


class ApprovalRequestParams(TypedDict, total=False):
    approvalId: str
    kind: str
    summary: str
    threadId: str
    turnId: str
    tool: str
    input: Any


class InitializeResponse(TypedDict, total=False):
    serverInfo: dict[str, str]
    zodeHome: str
    platformFamily: str
    platformOs: str
    capabilities: list[str]
    approvalPolicy: str
