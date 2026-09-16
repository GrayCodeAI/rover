// Package mcp implements a bounded JSON-RPC MCP 2025-11-25 tool surface over
// stdio and stateless HTTP. No sampling, task extension, OAuth discovery or
// implicit review/merge authority is advertised.
package mcp

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/GrayCodeAI/rover/internal/access"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/service"
	"github.com/GrayCodeAI/rover/internal/wire"
	"strconv"
)

const Protocol = "2025-11-25"
const MaxMessage = 1 << 20

type Request struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      json.RawMessage `json:"id,omitempty"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params,omitempty"`
}
type RPCError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}
type Response struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      json.RawMessage `json:"id"`
	Result  any             `json:"result,omitempty"`
	Error   *RPCError       `json:"error,omitempty"`
}

func failure(id json.RawMessage, code int, message string) Response {
	return Response{JSONRPC: "2.0", ID: id, Error: &RPCError{code, message}}
}
func decode(b []byte) (Request, error) {
	var r Request
	if len(b) > MaxMessage {
		return r, errors.New("message size limit exceeded")
	}
	if e := wire.Decode(b, &r); e != nil {
		return r, e
	}
	if r.JSONRPC != "2.0" || r.Method == "" {
		return r, errors.New("invalid JSON-RPC envelope")
	}
	if len(r.ID) > 0 {
		if _, e := idKey(r.ID); e != nil {
			return r, e
		}
	}
	return r, nil
}
func idKey(b []byte) (string, error) {
	if len(b) == 0 {
		return "", errors.New("notification has no id")
	}
	if b[0] == '"' {
		var s string
		if json.Unmarshal(b, &s) != nil || len(s) > 256 {
			return "", errors.New("invalid request id")
		}
		return "s:" + s, nil
	}
	n, e := strconv.ParseInt(string(b), 10, 64)
	if e != nil {
		return "", errors.New("request id must be a string or integer")
	}
	return "n:" + strconv.FormatInt(n, 10), nil
}
func params(r Request, v any) error {
	b := r.Params
	if len(b) == 0 {
		b = []byte("{}")
	}
	return wire.Decode(b, v)
}
func handle(ctx context.Context, s *service.Service, r Request, g *access.Grant, initialized bool) Response {
	result := func(v any) Response { return Response{JSONRPC: "2.0", ID: r.ID, Result: v} }
	switch r.Method {
	case "initialize":
		var p struct {
			ProtocolVersion string          `json:"protocolVersion"`
			Capabilities    json.RawMessage `json:"capabilities"`
			ClientInfo      json.RawMessage `json:"clientInfo"`
			Meta            json.RawMessage `json:"_meta,omitempty"`
		}
		if e := params(r, &p); e != nil || p.ProtocolVersion == "" || len(p.ClientInfo) == 0 || len(p.Capabilities) == 0 {
			return failure(r.ID, -32602, "initialize requires protocolVersion, capabilities and clientInfo")
		}
		var ci map[string]json.RawMessage
		var caps map[string]json.RawMessage
		if json.Unmarshal(p.ClientInfo, &ci) != nil || ci == nil || json.Unmarshal(p.Capabilities, &caps) != nil || caps == nil {
			return failure(r.ID, -32602, "clientInfo and capabilities must be objects")
		}
		var cname, cversion string
		if json.Unmarshal(ci["name"], &cname) != nil || json.Unmarshal(ci["version"], &cversion) != nil || cname == "" || cversion == "" {
			return failure(r.ID, -32602, "client name and version required")
		}
		return result(map[string]any{"protocolVersion": Protocol, "capabilities": map[string]any{"tools": map[string]any{"listChanged": false}}, "serverInfo": map[string]string{"name": "rover", "version": model.Version}, "instructions": "Results identify exact candidates. Tool execution success is not software acceptance. Review and policy authority are not exposed through MCP."})
	case "ping":
		return result(map[string]any{})
	}
	if !initialized {
		return failure(r.ID, -32002, "initialize and notifications/initialized required")
	}
	switch r.Method {
	case "tools/list":
		var p struct {
			Cursor string          `json:"cursor,omitempty"`
			Meta   json.RawMessage `json:"_meta,omitempty"`
		}
		if e := params(r, &p); e != nil || p.Cursor != "" {
			return failure(r.ID, -32602, "invalid cursor or parameters")
		}
		return result(map[string]any{"tools": s.Tools(g)})
	case "tools/call":
		var p struct {
			Name      string          `json:"name"`
			Arguments json.RawMessage `json:"arguments,omitempty"`
			Meta      json.RawMessage `json:"_meta,omitempty"`
		}
		if e := params(r, &p); e != nil || p.Name == "" {
			return failure(r.ID, -32602, "invalid tool call")
		}
		v, e := s.Call(ctx, p.Name, p.Arguments, g)
		if e != nil {
			return result(map[string]any{"content": []map[string]string{{"type": "text", "text": e.Error()}}, "isError": true})
		}
		b, e := json.Marshal(v)
		if e != nil {
			return failure(r.ID, -32603, "result encoding failed")
		}
		if len(b) > 8<<20 {
			return failure(r.ID, -32603, "result exceeds response budget")
		}
		var obj map[string]any
		if json.Unmarshal(b, &obj) != nil {
			obj = map[string]any{"result": v}
		}
		return result(map[string]any{"content": []map[string]string{{"type": "text", "text": string(b)}}, "structuredContent": obj, "isError": false})
	default:
		return failure(r.ID, -32601, "method not supported")
	}
}
func cancelledID(r Request) (string, error) {
	var p struct {
		RequestID json.RawMessage `json:"requestId"`
		Reason    string          `json:"reason,omitempty"`
	}
	if e := params(r, &p); e != nil {
		return "", e
	}
	return idKey(bytes.TrimSpace(p.RequestID))
}
func errText(e error) string { return fmt.Sprintf("protocol error: %v", e) }
