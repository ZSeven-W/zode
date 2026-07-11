use crate::rpc::RequestId;
use crate::types::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "camelCase")]
pub enum ClientRequest {
    #[serde(rename = "initialize")]
    Initialize {
        id: RequestId,
        params: InitializeParams,
    },
    #[serde(rename = "thread/start")]
    ThreadStart {
        id: RequestId,
        params: ThreadStartParams,
    },
    #[serde(rename = "thread/resume")]
    ThreadResume {
        id: RequestId,
        params: ThreadRefParams,
    },
    #[serde(rename = "thread/list")]
    ThreadList {
        id: RequestId,
        params: EmptyResponse,
    },
    #[serde(rename = "thread/read")]
    ThreadRead {
        id: RequestId,
        params: ThreadRefParams,
    },
    #[serde(rename = "thread/delete")]
    ThreadDelete {
        id: RequestId,
        params: ThreadRefParams,
    },
    #[serde(rename = "thread/name/set")]
    ThreadNameSet {
        id: RequestId,
        params: ThreadNameSetParams,
    },
    #[serde(rename = "turn/start")]
    TurnStart {
        id: RequestId,
        params: TurnStartParams,
    },
    #[serde(rename = "turn/interrupt")]
    TurnInterrupt {
        id: RequestId,
        params: TurnInterruptParams,
    },
    #[serde(rename = "fs/readFile")]
    FsReadFile {
        id: RequestId,
        params: FsReadFileParams,
    },
    #[serde(rename = "fs/writeFile")]
    FsWriteFile {
        id: RequestId,
        params: FsWriteFileParams,
    },
    #[serde(rename = "fs/createDirectory")]
    FsCreateDirectory {
        id: RequestId,
        params: FsCreateDirectoryParams,
    },
    #[serde(rename = "fs/getMetadata")]
    FsGetMetadata {
        id: RequestId,
        params: FsGetMetadataParams,
    },
    #[serde(rename = "fs/readDirectory")]
    FsReadDirectory {
        id: RequestId,
        params: FsReadDirectoryParams,
    },
    #[serde(rename = "fs/remove")]
    FsRemove {
        id: RequestId,
        params: FsRemoveParams,
    },
    #[serde(rename = "fs/copy")]
    FsCopy { id: RequestId, params: FsCopyParams },
    #[serde(rename = "command/exec")]
    CommandExec {
        id: RequestId,
        params: CommandExecParams,
    },
    #[serde(rename = "model/list")]
    ModelList {
        id: RequestId,
        params: EmptyResponse,
    },
    #[serde(rename = "model/set")]
    ModelSet {
        id: RequestId,
        params: ModelSetParams,
    },
    #[serde(rename = "config/read")]
    ConfigRead {
        id: RequestId,
        params: EmptyResponse,
    },
    #[serde(rename = "config/list")]
    ConfigList {
        id: RequestId,
        params: EmptyResponse,
    },
    #[serde(rename = "config/write")]
    ConfigWrite {
        id: RequestId,
        params: ConfigWriteParams,
    },
    #[serde(rename = "skills/list")]
    SkillsList {
        id: RequestId,
        params: EmptyResponse,
    },
    #[serde(rename = "skills/read")]
    SkillsRead {
        id: RequestId,
        params: SkillReadParams,
    },
    #[serde(rename = "hooks/list")]
    HooksList {
        id: RequestId,
        params: EmptyResponse,
    },
    #[serde(rename = "mcpServerStatus/list")]
    McpServerStatusList {
        id: RequestId,
        params: EmptyResponse,
    },
    #[serde(rename = "plugin/list")]
    PluginList {
        id: RequestId,
        params: EmptyResponse,
    },
}

impl ClientRequest {
    pub const ALL: &'static [&'static str] = &[
        "initialize",
        "thread/start",
        "thread/resume",
        "thread/list",
        "thread/read",
        "thread/delete",
        "thread/name/set",
        "turn/start",
        "turn/interrupt",
        "fs/readFile",
        "fs/writeFile",
        "fs/createDirectory",
        "fs/getMetadata",
        "fs/readDirectory",
        "fs/remove",
        "fs/copy",
        "command/exec",
        "model/list",
        "model/set",
        "config/read",
        "config/list",
        "config/write",
        "skills/list",
        "skills/read",
        "hooks/list",
        "mcpServerStatus/list",
        "plugin/list",
    ];

    pub fn id(&self) -> &RequestId {
        match self {
            Self::Initialize { id, .. }
            | Self::ThreadStart { id, .. }
            | Self::ThreadResume { id, .. }
            | Self::ThreadList { id, .. }
            | Self::ThreadRead { id, .. }
            | Self::ThreadDelete { id, .. }
            | Self::ThreadNameSet { id, .. }
            | Self::TurnStart { id, .. }
            | Self::TurnInterrupt { id, .. }
            | Self::FsReadFile { id, .. }
            | Self::FsWriteFile { id, .. }
            | Self::FsCreateDirectory { id, .. }
            | Self::FsGetMetadata { id, .. }
            | Self::FsReadDirectory { id, .. }
            | Self::FsRemove { id, .. }
            | Self::FsCopy { id, .. }
            | Self::CommandExec { id, .. }
            | Self::ModelList { id, .. }
            | Self::ModelSet { id, .. }
            | Self::ConfigRead { id, .. }
            | Self::ConfigList { id, .. }
            | Self::ConfigWrite { id, .. }
            | Self::SkillsList { id, .. }
            | Self::SkillsRead { id, .. }
            | Self::HooksList { id, .. }
            | Self::McpServerStatusList { id, .. }
            | Self::PluginList { id, .. } => id,
        }
    }
}
