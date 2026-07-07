package zodesdk

import "testing"

func TestDefaultBinary(t *testing.T) {
	client := NewClient(ClientOptions{})
	if client.Binary() != "zode" {
		t.Fatalf("expected zode, got %q", client.Binary())
	}
}

func TestBinaryOverride(t *testing.T) {
	client := NewClient(ClientOptions{Binary: "/tmp/zode"})
	if client.Binary() != "/tmp/zode" {
		t.Fatalf("expected override, got %q", client.Binary())
	}
}

func TestProtocolMethodWireNames(t *testing.T) {
	if ProtocolMethodInitialize.String() != "initialize" {
		t.Fatalf("unexpected initialize method %q", ProtocolMethodInitialize.String())
	}
	if ProtocolMethodCommandExec.String() != "command/exec" {
		t.Fatalf("unexpected command method %q", ProtocolMethodCommandExec.String())
	}
	if ProtocolMethodMcpServerStatusList.String() != "mcpServerStatus/list" {
		t.Fatalf("unexpected mcp method %q", ProtocolMethodMcpServerStatusList.String())
	}
}
