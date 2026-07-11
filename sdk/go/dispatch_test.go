package zodesdk

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"
)

// scriptedChild writes an executable /bin/sh stub into a unique subdirectory of
// t.TempDir() and returns its path, so it can stand in for the zode binary.
func scriptedChild(t *testing.T, body string) string {
	t.Helper()
	dir, err := os.MkdirTemp(t.TempDir(), "child")
	if err != nil {
		t.Fatalf("mkdir temp child: %v", err)
	}
	path := filepath.Join(dir, "zode")
	if err := os.WriteFile(path, []byte("#!/bin/sh\n"+body+"\n"), 0o755); err != nil {
		t.Fatalf("write child: %v", err)
	}
	return path
}

type tagResult struct {
	Tag string `json:"tag"`
}

func TestDispatchesNotificationsWhileResolvingOutOfOrder(t *testing.T) {
	// Child waits for both requests, emits a notification, then answers in
	// reverse arrival order, echoing each request's own tag so the assertion
	// proves the pending map routes by id, not by arrival order.
	child := scriptedChild(t, `
read line1
read line2
id1=$(printf '%s' "$line1" | sed 's/.*"id":\([0-9]*\).*/\1/')
tag1=$(printf '%s' "$line1" | sed 's/.*"tag":"\([^"]*\)".*/\1/')
id2=$(printf '%s' "$line2" | sed 's/.*"id":\([0-9]*\).*/\1/')
tag2=$(printf '%s' "$line2" | sed 's/.*"tag":"\([^"]*\)".*/\1/')
printf '{"jsonrpc":"2.0","method":"turn/started","params":{"turnId":"t"}}\n'
printf '{"jsonrpc":"2.0","id":%s,"result":{"tag":"%s"}}\n' "$id2" "$tag2"
printf '{"jsonrpc":"2.0","id":%s,"result":{"tag":"%s"}}\n' "$id1" "$tag1"
`)
	client := NewClient(ClientOptions{Binary: child})
	defer client.Close()

	var notesMu sync.Mutex
	var notes []string
	client.OnNotification(func(method string, _ json.RawMessage) {
		notesMu.Lock()
		notes = append(notes, method)
		notesMu.Unlock()
	})

	ctx := context.Background()
	type outcome struct {
		tag string
		err error
	}
	run := func(tag string) <-chan outcome {
		ch := make(chan outcome, 1)
		go func() {
			var res tagResult
			err := client.Request(ctx, "req", map[string]any{"tag": tag}, &res)
			ch <- outcome{tag: res.Tag, err: err}
		}()
		return ch
	}
	oneCh := run("one")
	twoCh := run("two")

	one := <-oneCh
	two := <-twoCh
	if one.err != nil || two.err != nil {
		t.Fatalf("request errors: one=%v two=%v", one.err, two.err)
	}
	if one.tag != "one" {
		t.Fatalf("request one got tag %q", one.tag)
	}
	if two.tag != "two" {
		t.Fatalf("request two got tag %q", two.tag)
	}

	notesMu.Lock()
	defer notesMu.Unlock()
	if len(notes) != 1 || notes[0] != "turn/started" {
		t.Fatalf("expected [turn/started], got %v", notes)
	}
}

// approvalChild asks for approval then echoes whichever decision it received
// back into the pending request's result so the test can assert it.
const approvalChildBody = `
read request
printf '{"jsonrpc":"2.0","id":"approval-1","method":"approval/request","params":{"approvalId":"a1","kind":"command","summary":"run"}}\n'
read approval
case "$approval" in
  *'"decision":"allowAlways"'*) printf '{"jsonrpc":"2.0","id":1,"result":{"decision":"allowAlways"}}\n';;
  *'"decision":"allow"'*) printf '{"jsonrpc":"2.0","id":1,"result":{"decision":"allow"}}\n';;
  *'"decision":"deny"'*) printf '{"jsonrpc":"2.0","id":1,"result":{"decision":"deny"}}\n';;
  *) exit 2;;
esac
`

type decisionResult struct {
	Decision string `json:"decision"`
}

func runApproval(t *testing.T, handler ApprovalHandler, register bool) string {
	t.Helper()
	client := NewClient(ClientOptions{Binary: scriptedChild(t, approvalChildBody)})
	defer client.Close()
	if register {
		client.OnApprovalRequest(handler)
	}
	var res decisionResult
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := client.Request(ctx, "test", map[string]any{}, &res); err != nil {
		t.Fatalf("request: %v", err)
	}
	return res.Decision
}

func TestApprovalAllow(t *testing.T) {
	got := runApproval(t, func(p ApprovalRequestParams) ApprovalDecision {
		if p.ApprovalID != "a1" {
			t.Errorf("expected approvalId a1, got %q", p.ApprovalID)
		}
		return DecisionAllow
	}, true)
	if got != "allow" {
		t.Fatalf("expected allow, got %q", got)
	}
}

func TestApprovalDeny(t *testing.T) {
	got := runApproval(t, func(ApprovalRequestParams) ApprovalDecision {
		return DecisionDeny
	}, true)
	if got != "deny" {
		t.Fatalf("expected deny, got %q", got)
	}
}

func TestApprovalMissingHandlerDenies(t *testing.T) {
	got := runApproval(t, nil, false)
	if got != "deny" {
		t.Fatalf("expected deny for missing handler, got %q", got)
	}
}

func TestApprovalPanicDenies(t *testing.T) {
	got := runApproval(t, func(ApprovalRequestParams) ApprovalDecision {
		panic("handler blew up")
	}, true)
	if got != "deny" {
		t.Fatalf("expected deny for panicking handler, got %q", got)
	}
}

func TestInitializeIncludesExplicitApprovalPolicy(t *testing.T) {
	child := scriptedChild(t, `
read request
case "$request" in
  *'"jsonrpc":"2.0"'*'"approvalPolicy":"auto"'*) printf '{"jsonrpc":"2.0","id":1,"result":{"ok":true}}\n';;
  *) exit 2;;
esac
`)
	client := NewClient(ClientOptions{Binary: child})
	defer client.Close()
	var res map[string]any
	if err := client.Initialize(context.Background(), "test", "1", "auto", &res); err != nil {
		t.Fatalf("initialize: %v", err)
	}
	if res["ok"] != true {
		t.Fatalf("unexpected result %v", res)
	}
}

func TestInitializeOmitsApprovalPolicyByDefault(t *testing.T) {
	child := scriptedChild(t, `
read request
case "$request" in
  *'"approvalPolicy"'*) exit 2;;
  *'"jsonrpc":"2.0"'*) printf '{"jsonrpc":"2.0","id":1,"result":{"ok":true}}\n';;
  *) exit 2;;
esac
`)
	client := NewClient(ClientOptions{Binary: child})
	defer client.Close()
	var res map[string]any
	if err := client.Initialize(context.Background(), "test", "1", "", &res); err != nil {
		t.Fatalf("initialize: %v", err)
	}
	if res["ok"] != true {
		t.Fatalf("unexpected result %v", res)
	}
}
