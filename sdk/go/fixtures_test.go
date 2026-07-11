package zodesdk

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

// requestCase rebuilds a fixture request through the SDK's own serialization so
// the assertion proves wire parity, not that we re-declared a literal.
type requestCase struct {
	id     RequestID
	method ProtocolMethod
	params any
}

func requestCases() map[string]requestCase {
	return map[string]requestCase{
		"initialize.request": {
			id:     "init",
			method: ProtocolMethodInitialize,
			params: NewInitializeParams("fixture", "0.0.0", "readOnly"),
		},
		"thread-start.request": {
			id:     "thread",
			method: ProtocolMethodThreadStart,
			params: map[string]any{"cwd": "/tmp/project", "model": "default"},
		},
		"fs-read-file.request": {
			id:     "read",
			method: ProtocolMethodFsReadFile,
			params: map[string]any{"path": "/tmp/project/hello.txt"},
		},
		"command-exec.request": {
			id:     "cmd",
			method: ProtocolMethodCommandExec,
			params: map[string]any{"command": []string{"sh", "-c", "printf hi"}},
		},
	}
}

func TestEveryRequestFixtureIsCovered(t *testing.T) {
	dir := fixturesDir(t)
	matches, err := filepath.Glob(filepath.Join(dir, "*.request.json"))
	if err != nil {
		t.Fatal(err)
	}
	cases := requestCases()
	for _, path := range matches {
		stem := strings.TrimSuffix(filepath.Base(path), ".json")
		if _, ok := cases[stem]; !ok {
			t.Fatalf("uncovered request fixture: %s", stem)
		}
	}
}

func TestRequestFixturesMatchSDKSerialization(t *testing.T) {
	dir := fixturesDir(t)
	for stem, tc := range requestCases() {
		raw, err := os.ReadFile(filepath.Join(dir, stem+".json"))
		if err != nil {
			t.Fatalf("%s: %v", stem, err)
		}
		var expected any
		if err := json.Unmarshal(raw, &expected); err != nil {
			t.Fatalf("%s: parse fixture: %v", stem, err)
		}

		frame := JSONRPCRequest{JSONRPC: JSONRPCVersion, ID: tc.id, Method: tc.method.String(), Params: tc.params}
		builtRaw, err := json.Marshal(frame)
		if err != nil {
			t.Fatalf("%s: marshal frame: %v", stem, err)
		}
		var built any
		if err := json.Unmarshal(builtRaw, &built); err != nil {
			t.Fatalf("%s: parse built: %v", stem, err)
		}

		if !reflect.DeepEqual(built, expected) {
			t.Fatalf("%s: mismatch\n built:    %s\n expected: %s", stem, builtRaw, raw)
		}
	}
}

func TestResponseFixturesClassifyAsResponses(t *testing.T) {
	dir := fixturesDir(t)
	matches, err := filepath.Glob(filepath.Join(dir, "*.response.json"))
	if err != nil {
		t.Fatal(err)
	}
	if len(matches) == 0 {
		t.Fatal("no response fixtures found")
	}
	for _, path := range matches {
		raw, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("%s: %v", path, err)
		}
		frame, err := ClassifyIncomingFrame(raw)
		if err != nil {
			t.Fatalf("%s: classify: %v", path, err)
		}
		if frame.Kind != FrameResponse {
			t.Fatalf("%s: got kind %q", path, frame.Kind)
		}
	}
}
