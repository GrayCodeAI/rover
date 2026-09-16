package mcp

import (
	"bytes"
	"context"
	"crypto/tls"
	"crypto/x509"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"strings"
	"time"

	"github.com/GrayCodeAI/rover/internal/model"
)

const MaxResponseBytes = 16 << 20

type ToolContent struct {
	Type string `json:"type"`
	Text string `json:"text"`
}
type ToolResult struct {
	Content           []ToolContent   `json:"content"`
	StructuredContent json.RawMessage `json:"structuredContent,omitempty"`
	IsError           bool            `json:"isError"`
}

// Remote connects to Rover's JSON-only Streamable HTTP subset. It rejects
// plaintext bearer tokens off loopback, redirects, and insecure TLS overrides.
type Remote struct {
	URL    string
	Token  string
	Client *http.Client
}

func NewRemote(endpoint, token, caFile string) (*Remote, error) {
	u, e := url.Parse(endpoint)
	if e != nil || u.Host == "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" {
		return nil, errors.New("explicit endpoint without credentials/query/fragment required")
	}
	ip := net.ParseIP(u.Hostname())
	if u.Scheme != "https" && (u.Scheme != "http" || ip == nil || !ip.IsLoopback()) {
		return nil, errors.New("non-loopback endpoints require HTTPS")
	}
	if len(token) < 32 || len(token) > 1024 || strings.ContainsAny(token, "\r\n \t") {
		return nil, errors.New("invalid bearer token")
	}
	tc := &tls.Config{MinVersion: tls.VersionTLS13}
	if caFile != "" {
		st, e := os.Stat(caFile)
		if e != nil || !st.Mode().IsRegular() || st.Size() > 1<<20 {
			return nil, errors.New("invalid CA file")
		}
		b, e := os.ReadFile(caFile)
		if e != nil {
			return nil, e
		}
		pool := x509.NewCertPool()
		if !pool.AppendCertsFromPEM(b) {
			return nil, errors.New("CA file has no certificates")
		}
		tc.RootCAs = pool
	}
	tr := &http.Transport{TLSClientConfig: tc, Proxy: nil, DialContext: (&net.Dialer{Timeout: 10 * time.Second, KeepAlive: 30 * time.Second}).DialContext, ResponseHeaderTimeout: 15 * time.Minute, MaxConnsPerHost: 8, MaxIdleConns: 8, IdleConnTimeout: 30 * time.Second}
	return &Remote{URL: endpoint, Token: token, Client: &http.Client{Transport: tr, CheckRedirect: func(*http.Request, []*http.Request) error { return errors.New("redirect refused") }}}, nil
}
func (r *Remote) Close() { r.Client.CloseIdleConnections() }
func (r *Remote) request(ctx context.Context, method string, params any) (json.RawMessage, error) {
	id := model.ID("rpc")
	body, e := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": id, "method": method, "params": params})
	if e != nil {
		return nil, e
	}
	if len(body) > MaxMessage {
		return nil, errors.New("request too large")
	}
	req, e := http.NewRequestWithContext(ctx, http.MethodPost, r.URL, bytes.NewReader(body))
	if e != nil {
		return nil, e
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json, text/event-stream")
	req.Header.Set("Authorization", "Bearer "+r.Token)
	if method != "initialize" {
		req.Header.Set("MCP-Protocol-Version", Protocol)
	}
	resp, e := r.Client.Do(req)
	if e != nil {
		return nil, e
	}
	defer resp.Body.Close()
	b, e := io.ReadAll(io.LimitReader(resp.Body, MaxResponseBytes+1))
	if e != nil {
		return nil, e
	}
	if len(b) > MaxResponseBytes {
		return nil, errors.New("remote response exceeds bound")
	}
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("remote HTTP status %d; response not treated as evidence", resp.StatusCode)
	}
	var result struct {
		JSONRPC string          `json:"jsonrpc"`
		ID      string          `json:"id"`
		Result  json.RawMessage `json:"result"`
		Error   *RPCError       `json:"error"`
	}
	if e = json.Unmarshal(b, &result); e != nil {
		return nil, e
	}
	if result.JSONRPC != "2.0" || result.ID != id {
		return nil, errors.New("response identity mismatch")
	}
	if result.Error != nil {
		return nil, fmt.Errorf("remote RPC error %d: %s", result.Error.Code, result.Error.Message)
	}
	if len(result.Result) == 0 {
		return nil, errors.New("missing RPC result")
	}
	return result.Result, nil
}
func (r *Remote) Initialize(ctx context.Context) error {
	raw, e := r.request(ctx, "initialize", map[string]any{"protocolVersion": Protocol, "capabilities": map[string]any{}, "clientInfo": map[string]string{"name": "Rover client", "version": model.Version}})
	if e != nil {
		return e
	}
	var x struct {
		Protocol string `json:"protocolVersion"`
	}
	if e = json.Unmarshal(raw, &x); e != nil {
		return e
	}
	if x.Protocol != Protocol {
		return errors.New("unsupported negotiated MCP version")
	}
	return nil
}
func (r *Remote) Tools(ctx context.Context) (json.RawMessage, error) {
	if e := r.Initialize(ctx); e != nil {
		return nil, e
	}
	return r.request(ctx, "tools/list", map[string]any{})
}
func (r *Remote) Call(ctx context.Context, name string, args json.RawMessage) (ToolResult, error) {
	var tr ToolResult
	if e := r.Initialize(ctx); e != nil {
		return tr, e
	}
	if len(args) == 0 {
		args = json.RawMessage(`{}`)
	}
	if !json.Valid(args) {
		return tr, errors.New("invalid tool arguments")
	}
	b, e := r.request(ctx, "tools/call", map[string]any{"name": name, "arguments": args})
	if e != nil {
		return tr, e
	}
	e = json.Unmarshal(b, &tr)
	return tr, e
}
