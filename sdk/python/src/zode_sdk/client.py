from __future__ import annotations

import asyncio
import json
from dataclasses import dataclass, field
from typing import Any, TypeVar, cast

from .protocol import InitializeResponse, ProtocolMethod

T = TypeVar("T")


class RpcError(Exception):
    def __init__(self, error: dict[str, Any]):
        self.error = error
        super().__init__(str(error.get("message", "RPC error")))


@dataclass
class ZodeClient:
    binary: str = "zode"

    def async_client(self) -> "AsyncZodeClient":
        return AsyncZodeClient(binary=self.binary)


@dataclass
class AsyncZodeClient:
    binary: str = "zode"
    _process: asyncio.subprocess.Process | None = field(default=None, init=False)
    _next_id: int = field(default=1, init=False)

    async def start(self) -> None:
        if self._process is not None:
            return
        self._process = await asyncio.create_subprocess_exec(
            self.binary,
            "server",
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
        )

    async def initialize(
        self, name: str = "zode-sdk-python", version: str = "0.0.0"
    ) -> InitializeResponse:
        return await self.request(
            "initialize",
            {"clientInfo": {"name": name, "version": version}},
        )

    async def request(
        self, method: str | ProtocolMethod, params: dict[str, Any] | None = None
    ) -> T:
        await self.start()
        assert self._process is not None
        assert self._process.stdin is not None
        assert self._process.stdout is not None

        request_id = self._next_id
        self._next_id += 1
        payload = {"id": request_id, "method": str(method.value if isinstance(method, ProtocolMethod) else method)}
        if params is not None:
            payload["params"] = params
        self._process.stdin.write((json.dumps(payload) + "\n").encode())
        await self._process.stdin.drain()

        while True:
            line = await self._process.stdout.readline()
            if not line:
                raise RuntimeError("zode server closed the connection")
            message = json.loads(line)
            if message.get("id") != request_id:
                continue
            if "error" in message:
                raise RpcError(message["error"])
            return cast(T, message["result"])

    async def notify(
        self, method: str | ProtocolMethod, params: dict[str, Any] | None = None
    ) -> None:
        await self.start()
        assert self._process is not None
        assert self._process.stdin is not None
        payload = {"method": str(method.value if isinstance(method, ProtocolMethod) else method)}
        if params is not None:
            payload["params"] = params
        self._process.stdin.write((json.dumps(payload) + "\n").encode())
        await self._process.stdin.drain()

    async def close(self) -> None:
        if self._process is None:
            return
        self._process.terminate()
        try:
            await asyncio.wait_for(self._process.wait(), timeout=2)
        except asyncio.TimeoutError:
            self._process.kill()
            await self._process.wait()
        self._process = None
