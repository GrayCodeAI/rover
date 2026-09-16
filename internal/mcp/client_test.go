package mcp

import (
	"bytes"
	"context"
	"crypto/x509"
	"encoding/json"
	"encoding/pem"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/GrayCodeAI/rover/internal/access"
	"github.com/GrayCodeAI/rover/internal/assurance"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/service"
	"github.com/GrayCodeAI/rover/internal/testutil"
)

func executionService(t *testing.T, argv []string) *service.Service {
	t.Helper()
	cfg := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "check", Argv: argv, Parser: "exit-code", Required: true, Timeout: "10s"}}}
	b, _ := json.Marshal(cfg)
	repo, s := testutil.Repo(t, map[string]string{".rover/config.json": string(b), "file": "source"})
	sv, e := service.New(context.Background(), s, repo, "HEAD", assurance.Options{Mode: "local-advisory", AllowLocal: true}, true)
	if e != nil {
		t.Fatal(e)
	}
	return sv
}
func TestRemoteClientTLSAndToolGrant(t *testing.T) {
	sv := executionService(t, []string{"true"})
	g, token, e := access.Issue(sv.Store, sv.Repository, []string{"rover_status", "rover_agent_capabilities"}, "TLS fixture", time.Hour)
	if e != nil {
		t.Fatal(e)
	}
	srv := httptest.NewTLSServer(HTTP(sv))
	defer srv.Close()
	ca := filepath.Join(t.TempDir(), "ca.pem")
	cert, err := x509.ParseCertificate(srv.TLS.Certificates[0].Certificate[0])
	if err != nil {
		t.Fatal(err)
	}
	os.WriteFile(ca, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: cert.Raw}), 0600)
	remote, e := NewRemote(srv.URL+"/mcp", token, ca)
	if e != nil {
		t.Fatal(e)
	}
	defer remote.Close()
	out, e := remote.Call(context.Background(), "rover_status", json.RawMessage(`{}`))
	if e != nil || out.IsError {
		t.Fatal(out, e)
	}
	denied, e := remote.Call(context.Background(), "rover_verify", json.RawMessage(`{}`))
	if e != nil || !denied.IsError {
		t.Fatal("grant did not restrict tools", denied, e)
	}
	if e = access.Revoke(sv.Store, g.ID); e != nil {
		t.Fatal(e)
	}
	if _, e = remote.Tools(context.Background()); e == nil {
		t.Fatal("revoked remote token accepted")
	}
	if _, e = NewRemote("http://192.0.2.1:9000/mcp", token, ""); e == nil {
		t.Fatal("plaintext remote bearer allowed")
	}
	if _, e = NewRemote("https://example.invalid/mcp?token=x", token, ""); e == nil {
		t.Fatal("credential query accepted")
	}
}
func TestHTTPVerificationCancellationAndDuplicateID(t *testing.T) {
	sv := executionService(t, []string{"sh", "-c", "sleep 8"})
	_, token, e := access.Issue(sv.Store, sv.Repository, []string{"rover_inspect", "rover_verify"}, "cancel fixture", time.Hour)
	if e != nil {
		t.Fatal(e)
	}
	server := httptest.NewServer(HTTP(sv))
	defer server.Close()
	remote, e := NewRemote(server.URL+"/mcp", token, "")
	if e != nil {
		t.Fatal(e)
	}
	defer remote.Close()
	inspect, e := remote.Call(context.Background(), "rover_inspect", json.RawMessage(`{}`))
	if e != nil || inspect.IsError {
		t.Fatal(inspect, e)
	}
	var v struct {
		Candidate model.Snapshot `json:"candidate"`
	}
	raw, marshalErr := json.Marshal(inspect.StructuredContent)
	if marshalErr != nil {
		t.Fatal(marshalErr)
	}
	if e = json.Unmarshal(raw, &v); e != nil {
		t.Fatal(e)
	}
	body := []byte(fmt.Sprintf(`{"jsonrpc":"2.0","id":"long","method":"tools/call","params":{"name":"rover_verify","arguments":{"candidate_id":%q}}}`, v.Candidate.ID))
	send := func(b []byte) ([]byte, int, error) {
		req, _ := http.NewRequest(http.MethodPost, server.URL+"/mcp", bytes.NewReader(b))
		req.Header.Set("Authorization", "Bearer "+token)
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("MCP-Protocol-Version", Protocol)
		r, e := http.DefaultClient.Do(req)
		if e != nil {
			return nil, 0, e
		}
		defer r.Body.Close()
		var buf bytes.Buffer
		_, e = buf.ReadFrom(r.Body)
		return buf.Bytes(), r.StatusCode, e
	}
	done := make(chan []byte, 1)
	go func() { b, _, _ := send(body); done <- b }()
	deadline := time.Now().Add(3 * time.Second)
	running := false
	for time.Now().Before(deadline) {
		rows, _ := sv.Store.ListAll("investigation", 100)
		for _, raw := range rows {
			var in model.Investigation
			json.Unmarshal(raw, &in)
			if in.Decision == "PENDING" {
				running = true
			}
		}
		if running {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	if !running {
		t.Fatal("verification did not start")
	}
	duplicate, _, e := send(body)
	if e != nil || !bytes.Contains(duplicate, []byte("duplicate")) {
		t.Fatal(string(duplicate), e)
	}
	_, code, e := send([]byte(`{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"long","reason":"test cancellation"}}`))
	if e != nil || code != http.StatusAccepted {
		t.Fatal(code, e)
	}
	select {
	case result := <-done:
		if bytes.Contains(result, []byte(`"decision":"ACCEPTED"`)) {
			t.Fatal("cancelled check accepted")
		}
	case <-time.After(3 * time.Second):
		t.Fatal("HTTP cancellation did not terminate verification")
	}
}
func TestRemoteExecutionToolCatalog(t *testing.T) {
	sv := executionService(t, []string{"true"})
	_, token, e := access.Issue(sv.Store, sv.Repository, []string{"rover_task_run", "rover_status"}, "remote task fixture", time.Hour)
	if e != nil {
		t.Fatal(e)
	}
	server := httptest.NewServer(HTTP(sv))
	defer server.Close()
	r, e := NewRemote(server.URL+"/mcp", token, "")
	if e != nil {
		t.Fatal(e)
	}
	defer r.Close()
	// Package tests are not the CLI executable. Submit is exercised with a real
	// CLI by scripts/demo_extended.py; this test checks grant/catalog behavior.
	out, e := r.Tools(context.Background())
	if e != nil || !bytes.Contains(out, []byte("rover_task_run")) {
		t.Fatal(string(out), e)
	}
}
