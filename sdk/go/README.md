# Zode Go SDK

Go SDK for `zode server` stdio JSON-RPC.

## Install

Module path:

```go
github.com/ZSeven-W/zode/sdk/go
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
low-level JSON-RPC.
Every supported method's params, result shape, and constant name are documented
in the [SDK method reference](../README.md#method-reference).

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
