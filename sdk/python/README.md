# Zode Python SDK

Python SDK for `zode server` stdio JSON-RPC.

## Install

Install the wheel attached to the GitHub Release:

```sh
python3 -m pip install https://github.com/ZSeven-W/zode/releases/download/v0.2.0-beta.1/zode_sdk-0.2.0b1-py3-none-any.whl
```

For local development from this repository:

```sh
PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests
```

Package name:

```toml
zode-sdk
```

## Usage

`zode` must be on `PATH`, or construct `ZodeClient(binary="/absolute/path/to/zode")`.

```python
import asyncio
from zode_sdk import ProtocolMethod, ZodeClient


async def main() -> None:
    client = ZodeClient().async_client()
    try:
        init = await client.initialize("example", "0.1.0")
        print(init["serverInfo"]["name"])

        command = await client.request(
            ProtocolMethod.COMMAND_EXEC,
            {"command": ["sh", "-c", "printf hi"]},
        )
        print(command["stdout"])
    finally:
        await client.close()


asyncio.run(main())
```

`ZodeClient().async_client()` returns an `AsyncZodeClient`; all requests are
coroutines. Use `await client.request(ProtocolMethod.COMMAND_EXEC, params)` for
stable zode methods, or pass a raw string when you intentionally need low-level
JSON-RPC. Every supported method's params, result shape, and enum name are
documented in the [SDK method reference](../README.md#method-reference).

## Streaming turns and approvals

Register handlers before starting a turn. Pass `approval_policy="auto"` (or
`"prompt"` with an approval handler) so side-effecting work runs — the default
`readOnly` denies it.

```python
import asyncio
from zode_sdk import ProtocolMethod, ZodeClient


async def main() -> None:
    client = ZodeClient().async_client()

    def on_notification(frame: dict) -> None:
        if frame["method"] == "item/agentMessage/delta":
            print(frame["params"]["delta"], end="", flush=True)

    def on_approval(params: dict) -> str:
        print(f"approve {params['kind']}: {params['summary']}")
        return "allow"  # "allow" | "allowAlways" | "deny"

    client.on_notification(on_notification)
    client.on_approval_request(on_approval)

    try:
        await client.initialize("example", "0.1.0", approval_policy="auto")
        thread = await client.request(ProtocolMethod.THREAD_START, {})
        await client.request(
            ProtocolMethod.TURN_START,
            {"threadId": thread["thread"]["id"], "input": "list the repo files"},
        )
    finally:
        await client.close()


asyncio.run(main())
```

`on_notification` receives the raw JSON-RPC frame dict
(`{"jsonrpc", "method", "params"}`). The approval handler may be sync or
`async` and returns a decision string; an unregistered handler denies.

## Version

`zode-sdk` `0.2.0b1`.

## Test

```sh
PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests
```
