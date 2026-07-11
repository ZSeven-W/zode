package zodesdk

import (
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

// schemaPath resolves fixtures/jsonrpc/protocol.schema.json relative to this
// test file so the test does not depend on the working directory.
func fixturesDir(t *testing.T) string {
	t.Helper()
	_, thisFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("runtime.Caller failed")
	}
	// sdk/go/protocol_test.go -> sdk/fixtures/jsonrpc
	return filepath.Join(filepath.Dir(thisFile), "..", "fixtures", "jsonrpc")
}

func TestProtocolMethodsMatchSchema(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join(fixturesDir(t), "protocol.schema.json"))
	if err != nil {
		t.Fatalf("read schema: %v", err)
	}
	var schema struct {
		Methods []string `json:"methods"`
	}
	if err := json.Unmarshal(raw, &schema); err != nil {
		t.Fatalf("parse schema: %v", err)
	}
	if len(ProtocolMethods) != 27 {
		t.Fatalf("expected 27 methods, got %d", len(ProtocolMethods))
	}
	if len(schema.Methods) != len(ProtocolMethods) {
		t.Fatalf("schema has %d methods, SDK has %d", len(schema.Methods), len(ProtocolMethods))
	}
	for i, want := range schema.Methods {
		if got := ProtocolMethods[i].String(); got != want {
			t.Fatalf("method %d: schema %q, SDK %q", i, want, got)
		}
	}
}

func TestClassifyIncomingFrameKinds(t *testing.T) {
	cases := []struct {
		raw  string
		want FrameKind
	}{
		{`{"jsonrpc":"2.0","id":1,"result":{}}`, FrameResponse},
		{`{"jsonrpc":"2.0","id":1,"error":{"code":-1,"message":"x"}}`, FrameError},
		{`{"jsonrpc":"2.0","method":"turn/started","params":{}}`, FrameNotification},
		{`{"jsonrpc":"2.0","id":"a","method":"approval/request","params":{}}`, FrameServerRequest},
	}
	for _, tc := range cases {
		frame, err := ClassifyIncomingFrame([]byte(tc.raw))
		if err != nil {
			t.Fatalf("classify %s: unexpected error %v", tc.raw, err)
		}
		if frame.Kind != tc.want {
			t.Fatalf("classify %s: got %q want %q", tc.raw, frame.Kind, tc.want)
		}
	}
}

func TestClassifyRejectsFramesMissingJSONRPC(t *testing.T) {
	bad := []string{
		`{"id":1,"result":{}}`,
		`{"jsonrpc":"1.0","id":1,"result":{}}`,
		`{"jsonrpc":"2.0","id":1}`,
		`not json`,
	}
	for _, raw := range bad {
		if _, err := ClassifyIncomingFrame([]byte(raw)); err == nil {
			t.Fatalf("expected rejection for %q", raw)
		}
	}
}

func TestInitializeParamsOmitsEmptyApprovalPolicy(t *testing.T) {
	built, err := json.Marshal(NewInitializeParams("n", "v", ""))
	if err != nil {
		t.Fatal(err)
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(built, &fields); err != nil {
		t.Fatal(err)
	}
	if _, ok := fields["approvalPolicy"]; ok {
		t.Fatalf("empty approvalPolicy must be omitted, got %s", built)
	}
	built2, _ := json.Marshal(NewInitializeParams("n", "v", "auto"))
	if err := json.Unmarshal(built2, &fields); err != nil {
		t.Fatal(err)
	}
	if _, ok := fields["approvalPolicy"]; !ok {
		t.Fatalf("explicit approvalPolicy must be present, got %s", built2)
	}
}
