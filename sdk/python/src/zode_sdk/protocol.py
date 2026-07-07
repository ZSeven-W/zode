from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Any, TypedDict, Union

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
    FS_READ_FILE = "fs/readFile"
    FS_WRITE_FILE = "fs/writeFile"
    FS_CREATE_DIRECTORY = "fs/createDirectory"
    FS_GET_METADATA = "fs/getMetadata"
    FS_READ_DIRECTORY = "fs/readDirectory"
    FS_REMOVE = "fs/remove"
    FS_COPY = "fs/copy"
    COMMAND_EXEC = "command/exec"
    MODEL_LIST = "model/list"
    CONFIG_READ = "config/read"
    CONFIG_LIST = "config/list"
    SKILLS_LIST = "skills/list"
    SKILLS_READ = "skills/read"
    HOOKS_LIST = "hooks/list"
    MCP_SERVER_STATUS_LIST = "mcpServerStatus/list"
    PLUGIN_LIST = "plugin/list"


@dataclass
class JsonRpcRequest:
    id: RequestId
    method: str
    params: dict[str, Any] | None = None


class RpcErrorObject(TypedDict, total=False):
    code: int
    message: str
    data: Any


class InitializeResponse(TypedDict):
    serverInfo: dict[str, str]
    zodeHome: str
    platformFamily: str
    platformOs: str
    capabilities: list[str]
