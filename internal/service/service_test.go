package service

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/GrayCodeAI/rover/internal/access"
	"github.com/GrayCodeAI/rover/internal/assurance"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/testutil"
	"os"
	"path/filepath"
	"testing"
)

func TestServiceProjectBoundary(t *testing.T) {
	cfg := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "check", Argv: []string{"/usr/bin/true"}, Parser: "exit-code", Required: true, Timeout: "2s"}}}
	b, _ := json.Marshal(cfg)
	repo, s := testutil.Repo(t, map[string]string{".rover/config.json": string(b), "a": "private A"})
	svc, e := New(context.Background(), s, repo, "HEAD", assurance.Options{Mode: "local-advisory", AllowLocal: true}, false)
	if e != nil {
		t.Fatal(e)
	}
	if _, e = svc.Call(context.Background(), "rover_verify", []byte(`{}`), nil); !errors.Is(e, access.ErrDenied) {
		t.Fatal(e)
	}
	other := filepath.Join(t.TempDir(), "other")
	os.Mkdir(other, 0700)
	testutil.Git(t, other, "init", "-q")
	testutil.Git(t, other, "config", "user.email", "test@invalid")
	testutil.Git(t, other, "config", "user.name", "fixture")
	testutil.Write(t, other, "secret.txt", "private B")
	testutil.Commit(t, other)
	snap, e := source.Capture(context.Background(), s, other, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	args, _ := json.Marshal(map[string]any{"snapshot_id": snap.ID, "query": "private"})
	if _, e = svc.Call(context.Background(), "rover_context_search", args, nil); !errors.Is(e, access.ErrDenied) {
		t.Fatal("cross-project search", e)
	}
	args, _ = json.Marshal(map[string]any{"query": "private"})
	v, e := svc.Call(context.Background(), "rover_context_search", args, nil)
	if e != nil {
		t.Fatal(e)
	}
	out, _ := json.Marshal(v)
	if string(out) == "" {
		t.Fatal("no result")
	}
	if _, e = svc.Call(context.Background(), "rover_status", []byte(`{"TaskID":"x"}`), nil); e == nil {
		t.Fatal("noncanonical request accepted")
	}
}
