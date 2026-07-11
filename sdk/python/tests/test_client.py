import json
import pathlib
import unittest

import _support  # noqa: F401  (sys.path bootstrap)

from zode_sdk import ProtocolMethod, ZodeClient, classify_incoming_frame


class ZodeClientTests(unittest.TestCase):
    def test_defaults_to_zode_binary(self) -> None:
        client = ZodeClient()
        self.assertEqual(client.binary, "zode")

    def test_allows_binary_override(self) -> None:
        client = ZodeClient(binary="/tmp/zode")
        self.assertEqual(client.binary, "/tmp/zode")

    def test_protocol_method_enum_exposes_wire_names(self) -> None:
        self.assertEqual(ProtocolMethod.INITIALIZE.value, "initialize")
        self.assertEqual(ProtocolMethod.COMMAND_EXEC.value, "command/exec")
        self.assertEqual(
            ProtocolMethod.MCP_SERVER_STATUS_LIST.value,
            "mcpServerStatus/list",
        )

    def test_protocol_method_enum_covers_every_wire_method(self) -> None:
        schema_path = (
            pathlib.Path(__file__).resolve().parent.parent.parent
            / "fixtures"
            / "jsonrpc"
            / "protocol.schema.json"
        )
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
        wire_names = [member.value for member in ProtocolMethod]
        self.assertEqual(wire_names, schema["methods"])
        self.assertEqual(len(wire_names), 27)

    def test_classifies_every_incoming_frame_kind(self) -> None:
        self.assertEqual(
            classify_incoming_frame({"jsonrpc": "2.0", "id": 1, "result": {}}).kind,
            "response",
        )
        self.assertEqual(
            classify_incoming_frame(
                {"jsonrpc": "2.0", "id": 1, "error": {"code": -1, "message": "x"}}
            ).kind,
            "error",
        )
        self.assertEqual(
            classify_incoming_frame(
                {"jsonrpc": "2.0", "method": "turn/started", "params": {}}
            ).kind,
            "notification",
        )
        self.assertEqual(
            classify_incoming_frame(
                {"jsonrpc": "2.0", "id": "a", "method": "approval/request", "params": {}}
            ).kind,
            "serverRequest",
        )

    def test_classifier_rejects_frames_missing_jsonrpc(self) -> None:
        with self.assertRaises(ValueError):
            classify_incoming_frame({"id": 1, "result": {}})
        with self.assertRaises(ValueError):
            classify_incoming_frame({"jsonrpc": "1.0", "id": 1, "result": {}})
        with self.assertRaises(ValueError):
            classify_incoming_frame({"jsonrpc": "2.0", "id": 1})


if __name__ == "__main__":
    unittest.main()
