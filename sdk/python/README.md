# Zode Python SDK

Python SDK for `zode server` stdio JSON-RPC.

## Install

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

Use `await client.request(ProtocolMethod.COMMAND_EXEC, params)` for stable zode
methods, or pass a raw string when you intentionally need low-level JSON-RPC.
Every supported method's params, result shape, and enum name are documented in
the [SDK method reference](../README.md#method-reference).

## Test

```sh
PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests
```
