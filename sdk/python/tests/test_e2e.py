import asyncio
import json
import os
import tempfile
import unittest

import _support  # noqa: F401  (sys.path bootstrap)

from zode_sdk import AsyncZodeClient


class E2ETests(unittest.IsolatedAsyncioTestCase):
    async def test_stdio_basic_run(self) -> None:
        binary = os.environ.get("ZODE_BIN")
        if not binary:
            self.skipTest("ZODE_BIN unset; skipping stdio e2e")

        config_dir = tempfile.mkdtemp(prefix=f"zode-py-e2e-{os.getpid()}-")
        with open(
            os.path.join(config_dir, "config.json"), "w", encoding="utf-8"
        ) as handle:
            json.dump(
                {"provider": {"type": "anthropic"}, "sandbox": {"enabled": False}},
                handle,
            )

        # Isolate config and strip provider keys so no live turn can succeed;
        # we only assert the server reaches turn/failed, never a real answer.
        env = dict(os.environ)
        env["ZODE_CONFIG_DIR"] = config_dir
        env.pop("ANTHROPIC_API_KEY", None)
        env.pop("OPENAI_API_KEY", None)

        client = AsyncZodeClient(binary=binary, env=env)
        seen: list[str] = []
        client.on_notification(lambda frame: seen.append(frame["method"]))
        try:
            initialize = await client.initialize(
                "python-sdk-e2e", "0.1.0", approval_policy="auto"
            )
            self.assertEqual(initialize["approvalPolicy"], "auto")

            started = await client.request("thread/start", {"cwd": os.getcwd()})
            thread_id = started["thread"]["id"]

            await client.request(
                "turn/start", {"threadId": thread_id, "input": "echo hi"}
            )

            async def wait_for_turn() -> None:
                while not ("turn/started" in seen and "turn/failed" in seen):
                    await asyncio.sleep(0.01)

            await asyncio.wait_for(wait_for_turn(), timeout=10)

            command = await client.request(
                "command/exec", {"command": ["sh", "-c", "printf hi"]}
            )
            self.assertEqual(command["stdout"], "hi")
            self.assertEqual(command["exitCode"], 0)
        finally:
            await client.close()


if __name__ == "__main__":
    unittest.main()
