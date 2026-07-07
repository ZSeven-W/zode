package zodesdk

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"os/exec"
	"sync"
)

type ClientOptions struct {
	Binary string
}

type Client struct {
	binary string
	cmd    *exec.Cmd
	stdin  io.WriteCloser
	enc    *json.Encoder
	dec    *json.Decoder
	nextID int64
	mu     sync.Mutex
}

func NewClient(options ClientOptions) *Client {
	binary := options.Binary
	if binary == "" {
		binary = "zode"
	}
	return &Client{binary: binary, nextID: 1}
}

func (c *Client) Binary() string {
	return c.binary
}

func (c *Client) Start(ctx context.Context) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.cmd != nil {
		return nil
	}
	cmd := exec.CommandContext(ctx, c.binary, "server")
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
	c.enc = json.NewEncoder(stdin)
	c.dec = json.NewDecoder(stdout)
	return nil
}

func (c *Client) Request(ctx context.Context, method string, params any, result any) error {
	if err := c.Start(ctx); err != nil {
		return err
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.enc == nil || c.dec == nil {
		return errors.New("zode client is not started")
	}
	id := c.nextID
	c.nextID++
	if err := c.enc.Encode(JSONRPCRequest{ID: id, Method: method, Params: params}); err != nil {
		return err
	}
	for {
		var response JSONRPCResponse
		if err := c.dec.Decode(&response); err != nil {
			return err
		}
		if response.ID != float64(id) && response.ID != id {
			continue
		}
		if response.Error != nil {
			return response.Error
		}
		if result == nil {
			return nil
		}
		return json.Unmarshal(response.Result, result)
	}
}

func (c *Client) RequestMethod(ctx context.Context, method ProtocolMethod, params any, result any) error {
	return c.Request(ctx, method.String(), params, result)
}

func (c *Client) Notify(ctx context.Context, method string, params any) error {
	if err := c.Start(ctx); err != nil {
		return err
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.enc == nil {
		return errors.New("zode client is not started")
	}
	return c.enc.Encode(JSONRPCNotification{Method: method, Params: params})
}

func (c *Client) NotifyMethod(ctx context.Context, method ProtocolMethod, params any) error {
	return c.Notify(ctx, method.String(), params)
}

func (c *Client) Close() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.stdin != nil {
		_ = c.stdin.Close()
	}
	if c.cmd == nil || c.cmd.Process == nil {
		return nil
	}
	err := c.cmd.Process.Kill()
	_, _ = c.cmd.Process.Wait()
	c.cmd = nil
	c.stdin = nil
	c.enc = nil
	c.dec = nil
	return err
}
