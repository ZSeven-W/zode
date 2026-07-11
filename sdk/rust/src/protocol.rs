#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolMethod {
    Initialize,
    ThreadStart,
    ThreadResume,
    ThreadList,
    ThreadRead,
    ThreadDelete,
    ThreadNameSet,
    TurnStart,
    TurnInterrupt,
    FsReadFile,
    FsWriteFile,
    FsCreateDirectory,
    FsGetMetadata,
    FsReadDirectory,
    FsRemove,
    FsCopy,
    CommandExec,
    ModelList,
    ConfigRead,
    ConfigList,
    SkillsList,
    SkillsRead,
    HooksList,
    McpServerStatusList,
    PluginList,
}

impl ProtocolMethod {
    pub const ALL: &'static [ProtocolMethod] = &[
        Self::Initialize,
        Self::ThreadStart,
        Self::ThreadResume,
        Self::ThreadList,
        Self::ThreadRead,
        Self::ThreadDelete,
        Self::ThreadNameSet,
        Self::TurnStart,
        Self::TurnInterrupt,
        Self::FsReadFile,
        Self::FsWriteFile,
        Self::FsCreateDirectory,
        Self::FsGetMetadata,
        Self::FsReadDirectory,
        Self::FsRemove,
        Self::FsCopy,
        Self::CommandExec,
        Self::ModelList,
        Self::ConfigRead,
        Self::ConfigList,
        Self::SkillsList,
        Self::SkillsRead,
        Self::HooksList,
        Self::McpServerStatusList,
        Self::PluginList,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::ThreadStart => "thread/start",
            Self::ThreadResume => "thread/resume",
            Self::ThreadList => "thread/list",
            Self::ThreadRead => "thread/read",
            Self::ThreadDelete => "thread/delete",
            Self::ThreadNameSet => "thread/name/set",
            Self::TurnStart => "turn/start",
            Self::TurnInterrupt => "turn/interrupt",
            Self::FsReadFile => "fs/readFile",
            Self::FsWriteFile => "fs/writeFile",
            Self::FsCreateDirectory => "fs/createDirectory",
            Self::FsGetMetadata => "fs/getMetadata",
            Self::FsReadDirectory => "fs/readDirectory",
            Self::FsRemove => "fs/remove",
            Self::FsCopy => "fs/copy",
            Self::CommandExec => "command/exec",
            Self::ModelList => "model/list",
            Self::ConfigRead => "config/read",
            Self::ConfigList => "config/list",
            Self::SkillsList => "skills/list",
            Self::SkillsRead => "skills/read",
            Self::HooksList => "hooks/list",
            Self::McpServerStatusList => "mcpServerStatus/list",
            Self::PluginList => "plugin/list",
        }
    }
}

impl AsRef<str> for ProtocolMethod {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for ProtocolMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
