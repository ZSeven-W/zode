from __future__ import annotations

import asyncio
import inspect
import json
from dataclasses import dataclass, field
from typing import Any, Awaitable, Callable, Mapping, TypeVar, cast

from .protocol import (
    JSONRPC_VERSION,
    ClassifiedFrame,
    InitializeResponse,
    ProtocolMethod,
    RequestId,
    classify_incoming_frame,
    initialize_params,
    method_wire_name,
)

T = TypeVar("T")

# Notification handler receives the raw incoming frame dict (jsonrpc/method/params).
NotificationHandler = Callable[[dict[str, Any]], Any]
# Approval handler receives the approval params dict and returns a decision
# string ("allow" | "allowAlways" | "deny") or an awaitable of one.
ApprovalDecision = str
ApprovalHandler = Callable[[dict[str, Any]], "ApprovalDecision | Awaitable[ApprovalDecision]"]


class RpcError(Exception):
    def __init__(self, error: Mapping[str, Any]):
        self.error = dict(error)
        self.code = error.get("code")
        self.data = error.get("data")
        super().__init__(str(error.get("message", "RPC error")))


@dataclass
class ZodeClient:
    """Synchronous handle exposing the binary configuration. Call
    ``async_client()`` to obtain the asyncio dispatch client."""

    binary: str = "zode"
    server_args: tuple[str, ...] = ("server",)
    env: Mapping[str, str] | None = None

    def async_client(self) -> "AsyncZodeClient":
        return AsyncZodeClient(
            binary=self.binary, server_args=self.server_args, env=self.env
        )


@dataclass
class AsyncZodeClient:
    """Async JSON-RPC client over the ``zode server`` stdio transport.

    A single reader task owns the child's stdout, resolving pending request
    futures by id, dispatching notifications to ``on_notification``, and
    answering server->client ``approval/request`` frames via
    ``on_approval_request`` (each answered in its own task so the reader never
    blocks). All writes are serialized behind an ``asyncio.Lock``.
    """

    binary: str = "zode"
    server_args: tuple[str, ...] = ("server",)
    env: Mapping[str, str] | None = None

    _process: asyncio.subprocess.Process | None = field(default=None, init=False)
    _reader_task: asyncio.Task[None] | None = field(default=None, init=False)
    _write_lock: asyncio.Lock | None = field(default=None, init=False)
    _pending: dict[RequestId, asyncio.Future[Any]] = field(
        default_factory=dict, init=False
    )
    _next_id: int = field(default=1, init=False)
    _notification_handler: NotificationHandler | None = field(default=None, init=False)
    _approval_handler: ApprovalHandler | None = field(default=None, init=False)

    async def start(self) -> None:
        if self._process is not None:
            return
        self._write_lock = asyncio.Lock()
        self._process = await asyncio.create_subprocess_exec(
            self.binary,
            *self.server_args,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            env=dict(self.env) if self.env is not None else None,
        )
        self._reader_task = asyncio.create_task(self._read_loop())

    def on_notification(self, handler: NotificationHandler | None) -> Callable[[], None]:
        self._notification_handler = handler

        def unregister() -> None:
            if self._notification_handler is handler:
                self._notification_handler = None

        return unregister

    def on_approval_request(self, handler: ApprovalHandler | None) -> Callable[[], None]:
        self._approval_handler = handler

        def unregister() -> None:
            if self._approval_handler is handler:
                self._approval_handler = None

        return unregister

    async def initialize(
        self,
        name: str = "zode-sdk-python",
        version: str = "0.0.0",
        approval_policy: str | None = None,
    ) -> InitializeResponse:
        return await self.request(
            ProtocolMethod.INITIALIZE,
            initialize_params(name, version, approval_policy),
        )

    async def request(
        self, method: str | ProtocolMethod, params: dict[str, Any] | None = None
    ) -> T:
        await self.start()
        loop = asyncio.get_running_loop()
        request_id = self._next_id
        self._next_id += 1
        future: asyncio.Future[Any] = loop.create_future()
        self._pending[request_id] = future
        frame: dict[str, Any] = {
            "jsonrpc": JSONRPC_VERSION,
            "id": request_id,
            "method": method_wire_name(method),
        }
        if params is not None:
            frame["params"] = params
        try:
            await self._write(frame)
        except Exception:
            self._pending.pop(request_id, None)
            raise
        return cast(T, await future)

    async def notify(
        self, method: str | ProtocolMethod, params: dict[str, Any] | None = None
    ) -> None:
        await self.start()
        frame: dict[str, Any] = {
            "jsonrpc": JSONRPC_VERSION,
            "method": method_wire_name(method),
        }
        if params is not None:
            frame["params"] = params
        await self._write(frame)

    async def close(self) -> None:
        task = self._reader_task
        self._reader_task = None
        if task is not None:
            task.cancel()
            try:
                await task
            except (asyncio.CancelledError, Exception):
                pass
        process = self._process
        self._process = None
        if process is not None and process.returncode is None:
            process.terminate()
            try:
                await asyncio.wait_for(process.wait(), timeout=2)
            except asyncio.TimeoutError:
                process.kill()
                await process.wait()
        self._reject_pending(RuntimeError("zode client closed"))

    async def _write(self, value: dict[str, Any]) -> None:
        assert self._process is not None and self._process.stdin is not None
        assert self._write_lock is not None
        # Compact separators keep frames whitespace-free on the wire, matching
        # the other zode SDKs and the newline-delimited framing.
        data = (json.dumps(value, separators=(",", ":")) + "\n").encode()
        async with self._write_lock:
            self._process.stdin.write(data)
            await self._process.stdin.drain()

    async def _read_loop(self) -> None:
        assert self._process is not None and self._process.stdout is not None
        stdout = self._process.stdout
        try:
            while True:
                line = await stdout.readline()
                if not line:
                    break  # EOF: the server closed its stdout.
                try:
                    message = json.loads(line)
                except json.JSONDecodeError:
                    continue
                try:
                    classified = classify_incoming_frame(message)
                except ValueError:
                    # Reject frames that are not strict JSON-RPC 2.0.
                    continue
                self._route(classified)
        finally:
            self._reject_pending(RuntimeError("zode server closed the connection"))

    def _route(self, classified: ClassifiedFrame) -> None:
        kind, frame = classified.kind, classified.frame
        if kind in ("response", "error"):
            future = self._pending.pop(frame["id"], None)
            if future is None or future.done():
                return
            if kind == "response":
                future.set_result(frame["result"])
            else:
                future.set_exception(RpcError(frame["error"]))
        elif kind == "notification":
            handler = self._notification_handler
            if handler is not None:
                try:
                    handler(frame)
                except Exception:
                    # A raising notification handler must not kill the reader.
                    pass
        else:  # serverRequest
            # Answer in a separate task so an awaitable approval handler never
            # blocks the reader from processing further frames.
            asyncio.create_task(self._answer_server_request(frame))

    async def _answer_server_request(self, request: dict[str, Any]) -> None:
        if request.get("method") != "approval/request":
            await self._write(
                {
                    "jsonrpc": JSONRPC_VERSION,
                    "id": request["id"],
                    "error": {"code": -32601, "message": "method not found"},
                }
            )
            return
        decision: ApprovalDecision = "deny"
        handler = self._approval_handler
        if handler is not None:
            try:
                result = handler(request.get("params") or {})
                if inspect.isawaitable(result):
                    result = await result
                decision = cast(ApprovalDecision, result)
            except Exception:
                # An unregistered handler or one that raises both deny.
                decision = "deny"
        try:
            await self._write(
                {
                    "jsonrpc": JSONRPC_VERSION,
                    "id": request["id"],
                    "result": {"decision": decision},
                }
            )
        except Exception:
            pass

    def _reject_pending(self, error: Exception) -> None:
        pending = self._pending
        self._pending = {}
        for future in pending.values():
            if not future.done():
                future.set_exception(error)
