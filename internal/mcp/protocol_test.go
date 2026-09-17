package mcp

import (
	"bytes"
	"context"
	"encoding/json"
	"github.com/GrayCodeAI/rover/internal/access"
	"github.com/GrayCodeAI/rover/internal/assurance"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/service"
	"github.com/GrayCodeAI/rover/internal/testutil"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func fixture(t *testing.T, enable bool) *service.Service {
	t.Helper()
	cfg := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "check", Argv: []string{"/usr/bin/true"}, Parser: "exit-code", Required: true, Timeout: "2s"}}}
	b, _ := json.Marshal(cfg)
	repo, s := testutil.Repo(t, map[string]string{".rover/config.json": string(b), "a.txt": "needle"})
	svc, e := service.New(context.Background(), s, repo, "HEAD", assurance.Options{Mode: "local-advisory", AllowLocal: true}, enable)
	if e != nil {
		t.Fatal(e)
	}
	return svc
}
func TestStdioLifecycle(t *testing.T) {
	svc := fixture(t, false)
	inR, inW := io.Pipe()
	outR, outW := io.Pipe()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	done := make(chan error, 1)
	go func() { done <- ServeStdio(ctx, svc, inR, outW) }()
	enc, dec := json.NewEncoder(inW), json.NewDecoder(outR)
	send := func(v any) Response {
		t.Helper()
		if e := enc.Encode(v); e != nil {
			t.Fatal(e)
		}
		var r Response
		if e := dec.Decode(&r); e != nil {
			t.Fatal(e)
		}
		return r
	}
	r := send(map[string]any{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": map[string]any{"protocolVersion": Protocol, "capabilities": map[string]any{}, "clientInfo": map[string]string{"name": "test", "version": "1"}}})
	if r.Error != nil {
		t.Fatal(r)
	}
	r = send(map[string]any{"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
	if r.Error == nil {
		t.Fatal("used before initialized")
	}
	_ = enc.Encode(map[string]any{"jsonrpc": "2.0", "method": "notifications/initialized"})
	r = send(map[string]any{"jsonrpc": "2.0", "id": 3, "method": "tools/list"})
	b, _ := json.Marshal(r.Result)
	if r.Error != nil || !strings.Contains(string(b), "rover_inspect") || strings.Contains(string(b), "rover_verify") {
		t.Fatal(string(b), r.Error)
	}
	r = send(map[string]any{"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": map[string]any{"name": "rover_status", "arguments": map[string]any{}}})
	if r.Error != nil {
		t.Fatal(r)
	}
	inW.Close()
	if e := <-done; e != nil {
		t.Fatal(e)
	}
	outR.Close()
}
func TestHTTPGrantsProtocolAndOrigin(t *testing.T) {
	svc := fixture(t, false)
	g, token, e := access.Issue(svc.Store, svc.Repository, []string{"rover_status"}, "test", time.Hour)
	if e != nil {
		t.Fatal(e)
	}
	handler := HTTP(svc)
	call := func(body, auth, origin, version string) *httptest.ResponseRecorder {
		r := httptest.NewRequest("POST", "/mcp", strings.NewReader(body))
		r.Header.Set("Content-Type", "application/json")
		if auth != "" {
			r.Header.Set("Authorization", "Bearer "+auth)
		}
		if origin != "" {
			r.Header.Set("Origin", origin)
		}
		if version != "" {
			r.Header.Set("MCP-Protocol-Version", version)
		}
		w := httptest.NewRecorder()
		handler.ServeHTTP(w, r)
		return w
	}
	body := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"rover_status","arguments":{}}}`
	if w := call(body, "", "", Protocol); w.Code != http.StatusUnauthorized {
		t.Fatal(w.Code)
	}
	if w := call(body, token, "https://untrusted.invalid", Protocol); w.Code != http.StatusForbidden {
		t.Fatal(w.Code)
	}
	if w := call(body, token, "", ""); w.Code != http.StatusBadRequest {
		t.Fatal(w.Code)
	}
	if w := call(body, token, "", Protocol); w.Code != http.StatusOK || !strings.Contains(w.Body.String(), "structuredContent") {
		t.Fatal(w.Code, w.Body.String())
	}
	w := call(strings.Replace(body, "rover_status", "rover_inspect", 1), token, "", Protocol)
	if !strings.Contains(w.Body.String(), `"isError":true`) {
		t.Fatal(w.Body.String())
	}
	_ = access.Revoke(svc.Store, g.ID)
	if w := call(body, token, "", Protocol); w.Code != http.StatusUnauthorized {
		t.Fatal(w.Code)
	}
}
func TestHTTPActualNetwork(t *testing.T) {
	svc := fixture(t, false)
	_, token, e := access.Issue(svc.Store, svc.Repository, []string{"rover_status"}, "loopback test", time.Hour)
	if e != nil {
		t.Fatal(e)
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	ready := make(chan string, 1)
	done := make(chan error, 1)
	go func() {
		done <- RunHTTP(ctx, svc, "127.0.0.1:0", "", "", false, func(a string) error { ready <- a; return nil })
	}()
	address := <-ready
	b := []byte(`{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"rover_status","arguments":{}}}`)
	req, _ := http.NewRequest("POST", "http://"+address+"/mcp", bytes.NewReader(b))
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("MCP-Protocol-Version", Protocol)
	r, e := http.DefaultClient.Do(req)
	if e != nil {
		t.Fatal(e)
	}
	io.Copy(io.Discard, r.Body)
	r.Body.Close()
	if r.StatusCode != 200 {
		t.Fatal(r.StatusCode)
	}
	cancel()
	if e = <-done; e != nil {
		t.Fatal(e)
	}
	if e = RunHTTP(context.Background(), svc, "0.0.0.0:0", "", "", false, nil); e == nil {
		t.Fatal("public HTTP without TLS admitted")
	}
}
func FuzzEnvelope(f *testing.F) {
	f.Add([]byte(`{"jsonrpc":"2.0","id":1,"method":"ping"}`))
	f.Add([]byte(`null`))
	f.Fuzz(func(t *testing.T, b []byte) { _, _ = decode(b) })
}
