package zodesdk

import "encoding/json"

type RequestID any

type ProtocolMethod string

const (
	ProtocolMethodInitialize          ProtocolMethod = "initialize"
	ProtocolMethodThreadStart         ProtocolMethod = "thread/start"
	ProtocolMethodThreadResume        ProtocolMethod = "thread/resume"
	ProtocolMethodThreadList          ProtocolMethod = "thread/list"
	ProtocolMethodThreadRead          ProtocolMethod = "thread/read"
	ProtocolMethodThreadDelete        ProtocolMethod = "thread/delete"
	ProtocolMethodThreadNameSet       ProtocolMethod = "thread/name/set"
	ProtocolMethodTurnStart           ProtocolMethod = "turn/start"
	ProtocolMethodFsReadFile          ProtocolMethod = "fs/readFile"
	ProtocolMethodFsWriteFile         ProtocolMethod = "fs/writeFile"
	ProtocolMethodFsCreateDirectory   ProtocolMethod = "fs/createDirectory"
	ProtocolMethodFsGetMetadata       ProtocolMethod = "fs/getMetadata"
	ProtocolMethodFsReadDirectory     ProtocolMethod = "fs/readDirectory"
	ProtocolMethodFsRemove            ProtocolMethod = "fs/remove"
	ProtocolMethodFsCopy              ProtocolMethod = "fs/copy"
	ProtocolMethodCommandExec         ProtocolMethod = "command/exec"
	ProtocolMethodModelList           ProtocolMethod = "model/list"
	ProtocolMethodConfigRead          ProtocolMethod = "config/read"
	ProtocolMethodConfigList          ProtocolMethod = "config/list"
	ProtocolMethodSkillsList          ProtocolMethod = "skills/list"
	ProtocolMethodSkillsRead          ProtocolMethod = "skills/read"
	ProtocolMethodHooksList           ProtocolMethod = "hooks/list"
	ProtocolMethodMcpServerStatusList ProtocolMethod = "mcpServerStatus/list"
	ProtocolMethodPluginList          ProtocolMethod = "plugin/list"
)

func (m ProtocolMethod) String() string {
	return string(m)
}

type JSONRPCRequest struct {
	ID     RequestID `json:"id"`
	Method string    `json:"method"`
	Params any       `json:"params,omitempty"`
}

type JSONRPCNotification struct {
	Method string `json:"method"`
	Params any    `json:"params,omitempty"`
}

type JSONRPCResponse struct {
	ID     RequestID       `json:"id"`
	Result json.RawMessage `json:"result"`
	Error  *RPCErrorObject `json:"error,omitempty"`
}

type RPCErrorObject struct {
	Code    int             `json:"code"`
	Message string          `json:"message"`
	Data    json.RawMessage `json:"data,omitempty"`
}

func (e *RPCErrorObject) Error() string {
	return e.Message
}
