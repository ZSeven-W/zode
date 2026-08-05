# Zode TypeScript SDK

TypeScript SDK for `zode server` stdio JSON-RPC.

## Install

From GitHub Packages (authenticate npm with a token that has
`read:packages`, then configure the `@zseven-w` scope for
`https://npm.pkg.github.com`):

```ini
# ~/.npmrc
@zseven-w:registry=https://npm.pkg.github.com
//npm.pkg.github.com/:_authToken=${GITHUB_TOKEN}
```

```sh
npm install @zseven-w/zode-sdk@0.1.0
```

From this repository:

```sh
pnpm --dir sdk/typescript install
pnpm --dir sdk/typescript build
```

Package name:

```json
"@zseven-w/zode-sdk"
```

## Usage

`zode` must be on `PATH`, or pass `{ binary: "/absolute/path/to/zode" }`.

```ts
import { ProtocolMethod, ZodeClient } from "@zseven-w/zode-sdk";

const client = new ZodeClient();

try {
  const init = await client.initialize("example", "0.1.0");
  console.log(init.serverInfo.name);

  const command = await client.request(ProtocolMethod.CommandExec, {
    command: ["sh", "-c", "printf hi"],
  });
  console.log(command);
} finally {
  client.close();
}
```

`request(ProtocolMethod.CommandExec, params)` auto-starts `zode server` over
stdio. Raw string method names are still accepted for low-level JSON-RPC use.
Every supported method's params, result shape, and enum name are documented in
the [SDK method reference](../README.md#method-reference).

## Streaming turns and approvals

Register handlers before starting a turn. Pass `approvalPolicy: "auto"` (or
`"prompt"` with an approval handler) so side-effecting work runs — the default
`readOnly` denies it.

```ts
import { ProtocolMethod, ZodeClient } from "@zseven-w/zode-sdk";

const client = new ZodeClient();

client.onNotification((n) => {
  if (n.method === "item/agentMessage/delta") process.stdout.write(n.params.delta);
  if (n.method === "turn/completed") console.log("\nusage:", n.params.usage);
});
client.onApprovalRequest((params) => {
  console.error(`approve ${params.kind}: ${params.summary}`);
  return "allow"; // "allow" | "allowAlways" | "deny"
});

await client.initialize("example", "0.1.0", { approvalPolicy: "auto" });
const { thread } = await client.request(ProtocolMethod.ThreadStart, {});
await client.request(ProtocolMethod.TurnStart, {
  threadId: thread.id,
  input: "list the repo files",
});
```

`onNotification` receives the whole JSON-RPC notification frame
(`{ jsonrpc, method, params }`). `onApprovalRequest` may return the decision
synchronously or as a `Promise`; an unregistered handler denies.

## WebSocket (Node only)

The WebSocket transport uses the runtime `ws` dependency because zode
authenticates the upgrade with an `Authorization: Bearer` header. Browser
WebSocket APIs cannot set that header.

Read `port` and `token` from the server's `server.json` credentials file (see
the [WebSocket transport](../README.md#websocket-transport) section) and build
the URL from them:

```ts
const client = await ZodeClient.connectWebSocket({
  url: "ws://127.0.0.1:9876",
  token: "server-token",
});
```

## Version

`@zseven-w/zode-sdk` `0.1.0`.

## Test

```sh
pnpm --dir sdk/typescript test
```
