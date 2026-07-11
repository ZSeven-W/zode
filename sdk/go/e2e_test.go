package zodesdk

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

// TestStdioBasicRun is an opt-in end-to-end test against a real zode binary.
// It runs only when ZODE_BIN points at one. The config disables the sandbox and
// selects the anthropic provider, while the child env drops provider API keys
// so no live turn can succeed: we only assert the server reaches turn/failed.
func TestStdioBasicRun(t *testing.T) {
	binary := os.Getenv("ZODE_BIN")
	if binary == "" {
		t.Skip("ZODE_BIN unset; skipping stdio e2e")
	}

	configDir := t.TempDir()
	config := map[string]any{
		"provider": map[string]any{"type": "anthropic"},
		"sandbox":  map[string]any{"enabled": false},
	}
	raw, err := json.Marshal(config)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(configDir, "config.json"), raw, 0o644); err != nil {
		t.Fatal(err)
	}

	// Isolate config and strip provider keys so no live turn can succeed.
	var env []string
	for _, kv := range os.Environ() {
		if strings.HasPrefix(kv, "ANTHROPIC_API_KEY=") ||
			strings.HasPrefix(kv, "OPENAI_API_KEY=") ||
			strings.HasPrefix(kv, "ZODE_CONFIG_DIR=") {
			continue
		}
		env = append(env, kv)
	}
	env = append(env, "ZODE_CONFIG_DIR="+configDir)

	client := NewClient(ClientOptions{Binary: binary, Env: env})
	defer client.Close()

	var seenMu sync.Mutex
	seen := map[string]bool{}
	client.OnNotification(func(method string, _ json.RawMessage) {
		seenMu.Lock()
		seen[method] = true
		seenMu.Unlock()
	})

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	var initResp map[string]any
	if err := client.Initialize(ctx, "zode-sdk-go", "0.0.0", "auto", &initResp); err != nil {
		t.Fatalf("initialize: %v", err)
	}
	if initResp["approvalPolicy"] != "auto" {
		t.Fatalf("expected approvalPolicy auto, got %v", initResp["approvalPolicy"])
	}

	cwd, _ := os.Getwd()
	var started struct {
		Thread struct {
			ID string `json:"id"`
		} `json:"thread"`
	}
	if err := client.Request(ctx, ProtocolMethodThreadStart.String(), map[string]any{"cwd": cwd}, &started); err != nil {
		t.Fatalf("thread/start: %v", err)
	}
	if started.Thread.ID == "" {
		t.Fatal("thread/start returned empty thread id")
	}

	if err := client.Request(ctx, ProtocolMethodTurnStart.String(),
		map[string]any{"threadId": started.Thread.ID, "input": "echo hi"}, nil); err != nil {
		t.Fatalf("turn/start: %v", err)
	}

	deadline := time.Now().Add(15 * time.Second)
	for {
		seenMu.Lock()
		done := seen["turn/started"] && seen["turn/failed"]
		seenMu.Unlock()
		if done {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("did not observe turn/started + turn/failed; saw %v", seen)
		}
		time.Sleep(10 * time.Millisecond)
	}

	var command struct {
		Stdout   string `json:"stdout"`
		ExitCode int    `json:"exitCode"`
	}
	if err := client.Request(ctx, ProtocolMethodCommandExec.String(),
		map[string]any{"command": []string{"sh", "-c", "printf hi"}}, &command); err != nil {
		t.Fatalf("command/exec: %v", err)
	}
	if command.Stdout != "hi" || command.ExitCode != 0 {
		t.Fatalf("unexpected command result: %+v", command)
	}
}
