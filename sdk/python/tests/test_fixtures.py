import json
import pathlib
import unittest

import _support  # noqa: F401  (sys.path bootstrap)

from zode_sdk import (
    ProtocolMethod,
    classify_incoming_frame,
    initialize_params,
    request_frame,
)

FIXTURES_DIR = (
    pathlib.Path(__file__).resolve().parent.parent.parent / "fixtures" / "jsonrpc"
)

# Rebuild each request fixture through the SDK serialization helpers so the
# assertion proves wire parity, not just that we can re-declare a literal.
REQUEST_CASES = {
    "initialize.request": (
        "init",
        ProtocolMethod.INITIALIZE,
        initialize_params("fixture", "0.0.0", approval_policy="readOnly"),
    ),
    "thread-start.request": (
        "thread",
        ProtocolMethod.THREAD_START,
        {"cwd": "/tmp/project", "model": "default"},
    ),
    "fs-read-file.request": (
        "read",
        ProtocolMethod.FS_READ_FILE,
        {"path": "/tmp/project/hello.txt"},
    ),
    "command-exec.request": (
        "cmd",
        ProtocolMethod.COMMAND_EXEC,
        {"command": ["sh", "-c", "printf hi"]},
    ),
}


class FixtureParityTests(unittest.TestCase):
    def test_every_fixture_file_is_covered(self) -> None:
        # Guard against a new fixture landing that this test silently ignores.
        for path in sorted(FIXTURES_DIR.glob("*.json")):
            stem = path.name[: -len(".json")]
            if stem.endswith(".request"):
                self.assertIn(stem, REQUEST_CASES, f"uncovered request fixture: {stem}")

    def test_request_fixtures_match_sdk_serialization(self) -> None:
        for path in sorted(FIXTURES_DIR.glob("*.request.json")):
            stem = path.name[: -len(".json")]
            request_id, method, params = REQUEST_CASES[stem]
            expected = json.loads(path.read_text(encoding="utf-8"))
            built = request_frame(request_id, method, params)
            with self.subTest(fixture=stem):
                self.assertEqual(built, expected)

    def test_response_fixtures_classify_as_responses(self) -> None:
        for path in sorted(FIXTURES_DIR.glob("*.response.json")):
            frame = json.loads(path.read_text(encoding="utf-8"))
            with self.subTest(fixture=path.name):
                self.assertEqual(classify_incoming_frame(frame).kind, "response")


if __name__ == "__main__":
    unittest.main()
