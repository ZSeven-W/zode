import unittest

from zode_sdk import ProtocolMethod, ZodeClient


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


if __name__ == "__main__":
    unittest.main()
