# Zode Go SDK

Go SDK for `zode server` stdio JSON-RPC.

## Install

Module path:

```go
github.com/ZSeven-W/zode/sdk/go
```

Install the tagged module version:

```sh
go get github.com/ZSeven-W/zode/sdk/go@v0.2.0-beta.4
```

For local development, work inside `sdk/go`.

## Usage

`zode` must be on `PATH`, or pass `ClientOptions{Binary: "/absolute/path/to/zode"}`.

```go
package main

import (
	"context"
	"fmt"

	zodesdk "github.com/ZSeven-W/zode/sdk/go"
)

func main() {
	ctx := context.Background()
	client := zodesdk.NewClient(zodesdk.ClientOptions{})
	defer client.Close()

	var init map[string]any
	err := client.Request(ctx, "initialize", map[string]any{
		"clientInfo": map[string]string{"name": "example", "version": "0.1.0"},
	}, &init)
	if err != nil {
		panic(err)
	}
	fmt.Println(init["serverInfo"])

	var command map[string]any
	err = client.RequestMethod(ctx, zodesdk.ProtocolMethodCommandExec, map[string]any{
		"command": []string{"sh", "-c", "printf hi"},
	}, &command)
	if err != nil {
		panic(err)
	}
	fmt.Println(command["stdout"])
}
```

Use `RequestMethod` / `NotifyMethod` with `ProtocolMethod...` constants for
stable zode methods. `Request` / `Notify` still accept raw strings for
low-level JSON-RPC. `Initialize(ctx, name, version, approvalPolicy, &out)` is a
convenience over `Request` for the handshake (an empty `approvalPolicy` omits
the field so the server default applies).
Every supported method's params, result shape, and constant name are documented
in the [SDK method reference](../README.md#method-reference).

## Streaming turns and approvals

Register handlers before starting a turn. Pass `"auto"` as the approval policy
(or `"prompt"` with an approval handler) so side-effecting work runs — the
default `readOnly` denies it.

```go
client := zodesdk.NewClient(zodesdk.ClientOptions{})
defer client.Close()

client.OnNotification(func(method string, params json.RawMessage) {
	if method == "item/agentMessage/delta" {
		var p struct{ Delta string `json:"delta"` }
		_ = json.Unmarshal(params, &p)
		fmt.Print(p.Delta)
	}
})
client.OnApprovalRequest(func(params zodesdk.ApprovalRequestParams) zodesdk.ApprovalDecision {
	fmt.Fprintf(os.Stderr, "approve %s: %s\n", params.Kind, params.Summary)
	return zodesdk.DecisionAllow // DecisionAllow | DecisionAllowAlways | DecisionDeny
})

if err := client.Initialize(ctx, "example", "0.1.0", "auto", nil); err != nil {
	panic(err)
}
var thread struct{ Thread struct{ ID string `json:"id"` } `json:"thread"` }
if err := client.RequestMethod(ctx, zodesdk.ProtocolMethodThreadStart, map[string]any{}, &thread); err != nil {
	panic(err)
}
err := client.RequestMethod(ctx, zodesdk.ProtocolMethodTurnStart, map[string]any{
	"threadId": thread.Thread.ID,
	"input":    "list the repo files",
}, nil)
if err != nil {
	panic(err)
}
```

`OnNotification` receives `(method, rawParams)`; `OnApprovalRequest` returns an
`ApprovalDecision` constant. An unregistered or panicking approval handler
denies.

## Version

Versioned by the module-aware git tag `sdk/go/v0.2.0-beta.4` for module
`github.com/ZSeven-W/zode/sdk/go`.

## Test

```sh
cd sdk/go
go test ./...
```

If your environment has a mismatched `GOROOT`, use the matching toolchain root,
for example:

```sh
GOROOT=/usr/local/go go test ./...
```
