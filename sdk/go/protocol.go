package zodesdk

import (
	"encoding/json"
	"errors"
)

// JSONRPCVersion is the exact version string every frame must carry. The zode
// app-server is strict: outgoing frames must include it and incoming frames
// without it are rejected by ClassifyIncomingFrame.
const JSONRPCVersion = "2.0"

// RequestID is a JSON-RPC id, which the spec allows to be a number or a string.
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
	ProtocolMethodTurnInterrupt       ProtocolMethod = "turn/interrupt"
	ProtocolMethodFsReadFile          ProtocolMethod = "fs/readFile"
	ProtocolMethodFsWriteFile         ProtocolMethod = "fs/writeFile"
	ProtocolMethodFsCreateDirectory   ProtocolMethod = "fs/createDirectory"
	ProtocolMethodFsGetMetadata       ProtocolMethod = "fs/getMetadata"
	ProtocolMethodFsReadDirectory     ProtocolMethod = "fs/readDirectory"
	ProtocolMethodFsRemove            ProtocolMethod = "fs/remove"
	ProtocolMethodFsCopy              ProtocolMethod = "fs/copy"
	ProtocolMethodCommandExec         ProtocolMethod = "command/exec"
	ProtocolMethodModelList           ProtocolMethod = "model/list"
	ProtocolMethodModelSet            ProtocolMethod = "model/set"
	ProtocolMethodConfigRead          ProtocolMethod = "config/read"
	ProtocolMethodConfigList          ProtocolMethod = "config/list"
	ProtocolMethodConfigWrite         ProtocolMethod = "config/write"
	ProtocolMethodSkillsList          ProtocolMethod = "skills/list"
	ProtocolMethodSkillsRead          ProtocolMethod = "skills/read"
	ProtocolMethodHooksList           ProtocolMethod = "hooks/list"
	ProtocolMethodMcpServerStatusList ProtocolMethod = "mcpServerStatus/list"
	ProtocolMethodPluginList          ProtocolMethod = "plugin/list"
)

// ProtocolMethods lists every client->server method in the canonical wire
// order defined by fixtures/jsonrpc/protocol.schema.json. Tests assert this
// slice matches the schema exactly (27 methods).
var ProtocolMethods = []ProtocolMethod{
	ProtocolMethodInitialize,
	ProtocolMethodThreadStart,
	ProtocolMethodThreadResume,
	ProtocolMethodThreadList,
	ProtocolMethodThreadRead,
	ProtocolMethodThreadDelete,
	ProtocolMethodThreadNameSet,
	ProtocolMethodTurnStart,
	ProtocolMethodTurnInterrupt,
	ProtocolMethodFsReadFile,
	ProtocolMethodFsWriteFile,
	ProtocolMethodFsCreateDirectory,
	ProtocolMethodFsGetMetadata,
	ProtocolMethodFsReadDirectory,
	ProtocolMethodFsRemove,
	ProtocolMethodFsCopy,
	ProtocolMethodCommandExec,
	ProtocolMethodModelList,
	ProtocolMethodModelSet,
	ProtocolMethodConfigRead,
	ProtocolMethodConfigList,
	ProtocolMethodConfigWrite,
	ProtocolMethodSkillsList,
	ProtocolMethodSkillsRead,
	ProtocolMethodHooksList,
	ProtocolMethodMcpServerStatusList,
	ProtocolMethodPluginList,
}

func (m ProtocolMethod) String() string {
	return string(m)
}

// JSONRPCRequest is a client->server request. JSONRPC must always be set to
// JSONRPCVersion so the strict server accepts the frame.
type JSONRPCRequest struct {
	JSONRPC string    `json:"jsonrpc"`
	ID      RequestID `json:"id"`
	Method  string    `json:"method"`
	Params  any       `json:"params,omitempty"`
}

// JSONRPCNotification is a client->server notification (no id).
type JSONRPCNotification struct {
	JSONRPC string `json:"jsonrpc"`
	Method  string `json:"method"`
	Params  any    `json:"params,omitempty"`
}

// JSONRPCResponse is a server->client response. Retained for backward
// compatibility with earlier callers; the dispatch loop uses the classifier.
type JSONRPCResponse struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      RequestID       `json:"id"`
	Result  json.RawMessage `json:"result"`
	Error   *RPCErrorObject `json:"error,omitempty"`
}

type RPCErrorObject struct {
	Code    int             `json:"code"`
	Message string          `json:"message"`
	Data    json.RawMessage `json:"data,omitempty"`
}

func (e *RPCErrorObject) Error() string {
	return e.Message
}

// FrameKind classifies an incoming frame.
type FrameKind string

const (
	FrameResponse      FrameKind = "response"
	FrameError         FrameKind = "error"
	FrameNotification  FrameKind = "notification"
	FrameServerRequest FrameKind = "serverRequest"
)

// ClassifiedFrame is an incoming frame tagged with its JSON-RPC kind. Fields
// holds the raw decoded top-level members so the dispatch loop can pull id,
// result, error, method, and params without re-decoding.
type ClassifiedFrame struct {
	Kind   FrameKind
	Fields map[string]json.RawMessage
}

// ErrInvalidFrame is returned when a frame is not a strict JSON-RPC 2.0 object.
var ErrInvalidFrame = errors.New("invalid JSON-RPC 2.0 frame")

// ClassifyIncomingFrame decodes and classifies a single incoming frame,
// rejecting anything that is not a strict JSON-RPC 2.0 object. Any frame
// missing the "jsonrpc":"2.0" marker (or otherwise malformed) yields
// ErrInvalidFrame.
func ClassifyIncomingFrame(raw []byte) (ClassifiedFrame, error) {
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fields); err != nil {
		return ClassifiedFrame{}, ErrInvalidFrame
	}
	verRaw, ok := fields["jsonrpc"]
	if !ok {
		return ClassifiedFrame{}, ErrInvalidFrame
	}
	var version string
	if err := json.Unmarshal(verRaw, &version); err != nil || version != JSONRPCVersion {
		return ClassifiedFrame{}, ErrInvalidFrame
	}
	if _, hasMethod := fields["method"]; hasMethod {
		var method string
		if err := json.Unmarshal(fields["method"], &method); err != nil {
			return ClassifiedFrame{}, ErrInvalidFrame
		}
		if _, hasID := fields["id"]; hasID {
			return ClassifiedFrame{Kind: FrameServerRequest, Fields: fields}, nil
		}
		return ClassifiedFrame{Kind: FrameNotification, Fields: fields}, nil
	}
	if _, hasID := fields["id"]; !hasID {
		return ClassifiedFrame{}, ErrInvalidFrame
	}
	if _, hasError := fields["error"]; hasError {
		return ClassifiedFrame{Kind: FrameError, Fields: fields}, nil
	}
	if _, hasResult := fields["result"]; hasResult {
		return ClassifiedFrame{Kind: FrameResponse, Fields: fields}, nil
	}
	return ClassifiedFrame{}, ErrInvalidFrame
}

// ClientInfo identifies the SDK client during the initialize handshake.
type ClientInfo struct {
	Name    string `json:"name"`
	Version string `json:"version"`
}

// InitializeParams are the params for the initialize request. ApprovalPolicy is
// omitted from the wire when empty so the server applies its own default.
type InitializeParams struct {
	ClientInfo     ClientInfo `json:"clientInfo"`
	ApprovalPolicy string     `json:"approvalPolicy,omitempty"`
}

// NewInitializeParams builds initialize params. An empty approvalPolicy is
// omitted from the wire (server default applies).
func NewInitializeParams(name, version, approvalPolicy string) InitializeParams {
	return InitializeParams{
		ClientInfo:     ClientInfo{Name: name, Version: version},
		ApprovalPolicy: approvalPolicy,
	}
}

// ApprovalDecision is the answer to a server->client approval/request.
type ApprovalDecision string

const (
	DecisionAllow       ApprovalDecision = "allow"
	DecisionAllowAlways ApprovalDecision = "allowAlways"
	DecisionDeny        ApprovalDecision = "deny"
)

// ApprovalRequestParams are the params of a server->client approval/request.
type ApprovalRequestParams struct {
	ApprovalID string          `json:"approvalId"`
	Kind       string          `json:"kind"`
	Summary    string          `json:"summary"`
	ThreadID   string          `json:"threadId,omitempty"`
	TurnID     string          `json:"turnId,omitempty"`
	Tool       string          `json:"tool,omitempty"`
	Input      json.RawMessage `json:"input,omitempty"`
}
