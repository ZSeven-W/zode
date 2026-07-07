# Zode TypeScript SDK

TypeScript SDK for `zode server` stdio JSON-RPC.

## Install

From this repository:

```sh
pnpm --dir sdk/typescript install
pnpm --dir sdk/typescript build
```

Package name:

```json
"@zseven/zode-sdk"
```

## Usage

`zode` must be on `PATH`, or pass `{ binary: "/absolute/path/to/zode" }`.

```ts
import { ProtocolMethod, ZodeClient } from "@zseven/zode-sdk";

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

## Test

```sh
pnpm --dir sdk/typescript test
```
