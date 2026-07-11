package zodesdk

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os/exec"
	"sync"
)

// ClientOptions configures a Client.
type ClientOptions struct {
	Binary string
	// ServerArgs overrides the args passed to the binary (default {"server"}).
	ServerArgs []string
	// Env, when non-nil, replaces the child process environment.
	Env []string
}

// NotificationHandler receives a decoded notification: its method plus raw
// params (nil when the frame carried none).
type NotificationHandler func(method string, params json.RawMessage)

// ApprovalHandler answers a server->client approval/request. An unregistered
// handler, or one that panics, results in a "deny" decision.
type ApprovalHandler func(params ApprovalRequestParams) ApprovalDecision

// rpcResult carries either a request result or an error to a waiting caller.
type rpcResult struct {
	result json.RawMessage
	err    error
}

// Client is a JSON-RPC 2.0 client over the "zode server" stdio transport.
//
// A single background reader goroutine owns the child's stdout, resolving
// pending request channels by id (supporting out-of-order responses),
// dispatching notifications to the registered NotificationHandler, and
// answering server->client approval/request frames via the ApprovalHandler.
// Each approval is answered in its own goroutine so the reader never blocks.
// All stdin writes are serialized behind writeMu.
type Client struct {
	binary     string
	serverArgs []string
	env        []string

	// mu guards process lifecycle fields, the pending map, nextID, and the
	// handler slots.
	mu      sync.Mutex
	cmd     *exec.Cmd
	stdin   io.WriteCloser
	pending map[int64]chan rpcResult
	nextID  int64

	notificationHandler NotificationHandler
	approvalHandler     ApprovalHandler

	// writeMu serializes the actual stdin writes (request frames from callers
	// and approval answers from the reader's goroutines must not interleave).
	writeMu sync.Mutex
}

// NewClient creates a Client. The binary defaults to "zode" and the server
// args default to {"server"}.
func NewClient(options ClientOptions) *Client {
	binary := options.Binary
	if binary == "" {
		binary = "zode"
	}
	args := options.ServerArgs
	if args == nil {
		args = []string{"server"}
	}
	return &Client{
		binary:     binary,
		serverArgs: args,
		env:        options.Env,
		pending:    make(map[int64]chan rpcResult),
		nextID:     1,
	}
}

// Binary returns the configured server binary path.
func (c *Client) Binary() string {
	return c.binary
}

// OnNotification registers the notification handler, replacing any previous
// one. Pass nil to clear it.
func (c *Client) OnNotification(handler NotificationHandler) {
	c.mu.Lock()
	c.notificationHandler = handler
	c.mu.Unlock()
}

// OnApprovalRequest registers the approval handler, replacing any previous
// one. Pass nil to clear it (an unregistered handler denies).
func (c *Client) OnApprovalRequest(handler ApprovalHandler) {
	c.mu.Lock()
	c.approvalHandler = handler
	c.mu.Unlock()
}

// Start spawns the server process (idempotent) and launches the reader loop.
func (c *Client) Start(ctx context.Context) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.cmd != nil {
		return nil
	}
	cmd := exec.CommandContext(ctx, c.binary, c.serverArgs...)
	if c.env != nil {
		cmd.Env = c.env
	}
	stdin, err := cmd.StdinPipe()
	if err != nil {
		return err
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return err
	}
	if err := cmd.Start(); err != nil {
		return err
	}
	c.cmd = cmd
	c.stdin = stdin
	go c.readLoop(stdout)
	return nil
}

// Initialize sends the initialize handshake. approvalPolicy is optional: an
// empty string omits the field from the wire so the server default applies.
// The decoded result is written into result (may be nil to ignore it).
func (c *Client) Initialize(ctx context.Context, name, version, approvalPolicy string, result any) error {
	return c.Request(ctx, ProtocolMethodInitialize.String(), NewInitializeParams(name, version, approvalPolicy), result)
}

// Request sends a client->server request and blocks until the matching
// response (by id) arrives, the context is cancelled, or the connection
// closes. The decoded result is written into result (may be nil to ignore it).
func (c *Client) Request(ctx context.Context, method string, params any, result any) error {
	if err := c.Start(ctx); err != nil {
		return err
	}
	c.mu.Lock()
	id := c.nextID
	c.nextID++
	ch := make(chan rpcResult, 1)
	c.pending[id] = ch
	c.mu.Unlock()

	frame := JSONRPCRequest{JSONRPC: JSONRPCVersion, ID: id, Method: method, Params: params}
	if err := c.write(frame); err != nil {
		c.mu.Lock()
		delete(c.pending, id)
		c.mu.Unlock()
		return err
	}

	select {
	case res := <-ch:
		if res.err != nil {
			return res.err
		}
		if result == nil {
			return nil
		}
		return json.Unmarshal(res.result, result)
	case <-ctx.Done():
		c.mu.Lock()
		delete(c.pending, id)
		c.mu.Unlock()
		return ctx.Err()
	}
}

// RequestMethod is a typed-method convenience over Request.
func (c *Client) RequestMethod(ctx context.Context, method ProtocolMethod, params any, result any) error {
	return c.Request(ctx, method.String(), params, result)
}

// Notify sends a client->server notification (no id, no response awaited).
func (c *Client) Notify(ctx context.Context, method string, params any) error {
	if err := c.Start(ctx); err != nil {
		return err
	}
	return c.write(JSONRPCNotification{JSONRPC: JSONRPCVersion, Method: method, Params: params})
}

// NotifyMethod is a typed-method convenience over Notify.
func (c *Client) NotifyMethod(ctx context.Context, method ProtocolMethod, params any) error {
	return c.Notify(ctx, method.String(), params)
}

// Close terminates the server process and rejects any pending requests. It is
// safe to call multiple times.
func (c *Client) Close() error {
	c.mu.Lock()
	stdin := c.stdin
	cmd := c.cmd
	c.stdin = nil
	c.cmd = nil
	c.mu.Unlock()

	if stdin != nil {
		_ = stdin.Close()
	}
	var err error
	if cmd != nil && cmd.Process != nil {
		err = cmd.Process.Kill()
		_, _ = cmd.Process.Wait()
	}
	c.rejectPending(errors.New("zode client closed"))
	return err
}

// write marshals value, appends a newline, and writes it to stdin under
// writeMu so concurrent frames never interleave.
func (c *Client) write(value any) error {
	data, err := json.Marshal(value)
	if err != nil {
		return err
	}
	data = append(data, '\n')

	c.mu.Lock()
	w := c.stdin
	c.mu.Unlock()
	if w == nil {
		return errors.New("zode client is not started")
	}

	c.writeMu.Lock()
	defer c.writeMu.Unlock()
	_, err = w.Write(data)
	return err
}

// readLoop owns the child's stdout, classifying and routing each newline-
// delimited frame until EOF.
func (c *Client) readLoop(stdout io.Reader) {
	reader := bufio.NewReader(stdout)
	for {
		line, err := reader.ReadBytes('\n')
		if len(line) > 0 {
			c.route(line)
		}
		if err != nil {
			break
		}
	}
	c.rejectPending(errors.New("zode server closed the connection"))
}

// route classifies one frame and dispatches it. Malformed or non-JSON-RPC-2.0
// frames are dropped.
func (c *Client) route(line []byte) {
	frame, err := ClassifyIncomingFrame(line)
	if err != nil {
		return
	}
	switch frame.Kind {
	case FrameResponse, FrameError:
		var id int64
		if json.Unmarshal(frame.Fields["id"], &id) != nil {
			return // not one of our integer-keyed requests
		}
		c.mu.Lock()
		ch := c.pending[id]
		delete(c.pending, id)
		c.mu.Unlock()
		if ch == nil {
			return
		}
		if frame.Kind == FrameError {
			eobj := &RPCErrorObject{}
			if err := json.Unmarshal(frame.Fields["error"], eobj); err != nil {
				eobj = &RPCErrorObject{Message: "invalid error object"}
			}
			ch <- rpcResult{err: eobj}
		} else {
			ch <- rpcResult{result: frame.Fields["result"]}
		}
	case FrameNotification:
		c.dispatchNotification(frame)
	case FrameServerRequest:
		// Answer in its own goroutine so a slow/blocking approval handler
		// never stalls the reader.
		go c.answerServerRequest(frame)
	}
}

// dispatchNotification invokes the notification handler with recover so a
// panicking handler cannot kill the reader.
func (c *Client) dispatchNotification(frame ClassifiedFrame) {
	c.mu.Lock()
	handler := c.notificationHandler
	c.mu.Unlock()
	if handler == nil {
		return
	}
	var method string
	_ = json.Unmarshal(frame.Fields["method"], &method)
	params := frame.Fields["params"]
	defer func() { _ = recover() }()
	handler(method, params)
}

// answerServerRequest handles a server->client request. Only approval/request
// is supported; anything else gets a method-not-found error. An unregistered
// or panicking approval handler denies.
func (c *Client) answerServerRequest(frame ClassifiedFrame) {
	idRaw := frame.Fields["id"]
	var method string
	_ = json.Unmarshal(frame.Fields["method"], &method)
	if method != "approval/request" {
		_ = c.write(rawErrorFrame{
			JSONRPC: JSONRPCVersion,
			ID:      idRaw,
			Error:   RPCErrorObject{Code: -32601, Message: "method not found"},
		})
		return
	}
	decision := c.resolveDecision(frame.Fields["params"])
	_ = c.write(rawResultFrame{
		JSONRPC: JSONRPCVersion,
		ID:      idRaw,
		Result:  approvalResult{Decision: decision},
	})
}

// resolveDecision runs the approval handler under recover, denying when it is
// unregistered or panics.
func (c *Client) resolveDecision(rawParams json.RawMessage) (decision ApprovalDecision) {
	decision = DecisionDeny
	c.mu.Lock()
	handler := c.approvalHandler
	c.mu.Unlock()
	if handler == nil {
		return DecisionDeny
	}
	var params ApprovalRequestParams
	if len(rawParams) > 0 {
		_ = json.Unmarshal(rawParams, &params)
	}
	defer func() {
		if recover() != nil {
			decision = DecisionDeny
		}
	}()
	return handler(params)
}

// rejectPending drains the pending map and fails every waiting caller. Draining
// under the lock ensures each channel is delivered to exactly once even if the
// reader loop and Close both call this.
func (c *Client) rejectPending(err error) {
	c.mu.Lock()
	pending := c.pending
	c.pending = make(map[int64]chan rpcResult)
	c.mu.Unlock()
	for _, ch := range pending {
		ch <- rpcResult{err: err}
	}
}

// rawResultFrame / rawErrorFrame / approvalResult serialize server-request
// answers, preserving the incoming id verbatim (it may be a string).
type rawResultFrame struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      json.RawMessage `json:"id"`
	Result  any             `json:"result"`
}

type rawErrorFrame struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      json.RawMessage `json:"id"`
	Error   RPCErrorObject  `json:"error"`
}

type approvalResult struct {
	Decision ApprovalDecision `json:"decision"`
}
