import asyncio
import unittest

import _support  # noqa: F401  (sys.path bootstrap)
from _support import scripted_child

from zode_sdk import AsyncZodeClient


class DispatchTests(unittest.IsolatedAsyncioTestCase):
    async def test_dispatches_notifications_while_resolving_out_of_order(self) -> None:
        # Child reads both requests, emits a notification, then answers id 2
        # before id 1 to prove the pending map resolves by id, not arrival.
        binary = scripted_child(
            "read first\n"
            "read second\n"
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"turn/started\",\"params\":{\"turnId\":\"t\"}}'\n"
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"value\":\"second\"}}'\n"
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"value\":\"first\"}}'\n"
        )
        client = AsyncZodeClient(binary=binary)
        notifications: list[str] = []
        client.on_notification(lambda frame: notifications.append(frame["method"]))
        try:
            # Start up front, then schedule both requests as tasks so they are
            # in-flight concurrently (bare coroutines would not run until
            # awaited). Task creation order fixes id 1 -> "one", id 2 -> "two".
            await client.start()
            first = asyncio.create_task(client.request("one", {}))
            second = asyncio.create_task(client.request("two", {}))
            self.assertEqual(await second, {"value": "second"})
            self.assertEqual(await first, {"value": "first"})
            self.assertEqual(notifications, ["turn/started"])
        finally:
            await client.close()

    async def _run_approval(self, handler, register: bool) -> None:
        # Child asks for approval, echoes back whichever decision it received
        # into the pending request result so the test can assert it.
        binary = scripted_child(
            "read request\n"
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":\"approval-1\",\"method\":\"approval/request\",\"params\":{\"approvalId\":\"a1\",\"kind\":\"command\",\"summary\":\"run\"}}'\n"
            "read approval\n"
            'case "$approval" in\n'
            '  *\'"decision":"allow"\'*) printf \'%s\\n\' \'{"jsonrpc":"2.0","id":1,"result":{"decision":"allow"}}\';;\n'
            '  *\'"decision":"deny"\'*) printf \'%s\\n\' \'{"jsonrpc":"2.0","id":1,"result":{"decision":"deny"}}\';;\n'
            "  *) exit 2;;\n"
            "esac\n"
        )
        client = AsyncZodeClient(binary=binary)
        if register:
            client.on_approval_request(handler)
        try:
            result = await client.request("test", {})
            self.assertEqual(result, {"decision": self._expected})
        finally:
            await client.close()

    async def test_approval_allow(self) -> None:
        self._expected = "allow"

        async def handler(params):
            self.assertEqual(params["approvalId"], "a1")
            return "allow"

        await self._run_approval(handler, register=True)

    async def test_approval_handler_raising_denies(self) -> None:
        self._expected = "deny"

        def handler(_params):
            raise RuntimeError("no")

        await self._run_approval(handler, register=True)

    async def test_approval_missing_handler_denies(self) -> None:
        self._expected = "deny"
        await self._run_approval(None, register=False)

    async def test_initialize_includes_explicit_approval_policy(self) -> None:
        binary = scripted_child(
            "read request\n"
            'case "$request" in\n'
            '  *\'"jsonrpc":"2.0"\'*\'"approvalPolicy":"auto"\'*) printf \'%s\\n\' \'{"jsonrpc":"2.0","id":1,"result":{"ok":true}}\';;\n'
            "  *) exit 2;;\n"
            "esac\n"
        )
        client = AsyncZodeClient(binary=binary)
        try:
            result = await client.initialize("test", "1", approval_policy="auto")
            self.assertEqual(result, {"ok": True})
        finally:
            await client.close()

    async def test_initialize_omits_approval_policy_by_default(self) -> None:
        binary = scripted_child(
            "read request\n"
            'case "$request" in\n'
            '  *\'"approvalPolicy"\'*) exit 2;;\n'
            '  *\'"jsonrpc":"2.0"\'*) printf \'%s\\n\' \'{"jsonrpc":"2.0","id":1,"result":{"ok":true}}\';;\n'
            "  *) exit 2;;\n"
            "esac\n"
        )
        client = AsyncZodeClient(binary=binary)
        try:
            result = await client.initialize("test", "1")
            self.assertEqual(result, {"ok": True})
        finally:
            await client.close()


if __name__ == "__main__":
    unittest.main()
