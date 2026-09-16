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
	"strings"
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

// TestCrossProjectEvidenceDenial (A16) proves the evidence views are project-bound:
// report, diff and verify must refuse a foreign investigation/snapshot even when
// its records live in the same store.
func TestCrossProjectEvidenceDenial(t *testing.T) {
	cfg := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "check", Argv: []string{"/usr/bin/true"}, Parser: "exit-code", Required: true, Timeout: "2s"}}}
	b, _ := json.Marshal(cfg)
	repo, s := testutil.Repo(t, map[string]string{".rover/config.json": string(b), "a": "private A"})
	svc, e := New(context.Background(), s, repo, "HEAD", assurance.Options{Mode: "local-advisory", AllowLocal: true}, true)
	if e != nil {
		t.Fatal(e)
	}
	other := filepath.Join(t.TempDir(), "other")
	if e := os.Mkdir(other, 0700); e != nil {
		t.Fatal(e)
	}
	testutil.Git(t, other, "init", "-q")
	testutil.Git(t, other, "config", "user.email", "test@invalid")
	testutil.Git(t, other, "config", "user.name", "fixture")
	testutil.Write(t, other, "secret.txt", "private B")
	testutil.Commit(t, other)
	snap, e := source.Capture(context.Background(), s, other, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	foreign := model.Investigation{Schema: model.Schema, Repository: other, Base: snap.ID, Candidate: snap.ID}
	if e = s.Put("investigation", "inv_foreign", foreign, "fixture"); e != nil {
		t.Fatal(e)
	}
	for _, name := range []string{"rover_report", "rover_diff"} {
		args, _ := json.Marshal(map[string]any{"investigation_id": "inv_foreign"})
		if _, e = svc.Call(context.Background(), name, args, nil); !errors.Is(e, access.ErrDenied) {
			t.Fatalf("%s: expected ErrDenied, got %v", name, e)
		}
	}
	args, _ := json.Marshal(map[string]any{"candidate_id": snap.ID})
	if _, e = svc.Call(context.Background(), "rover_verify", args, nil); !errors.Is(e, access.ErrDenied) {
		t.Fatalf("rover_verify: expected ErrDenied for foreign candidate, got %v", e)
	}
}

// TestToolSurfaceExcludesAuthority (A17) proves no tool name can reach
// review/approval/grants/signatures/policy/credentials or destructive commands.
func TestToolSurfaceExcludesAuthority(t *testing.T) {
	cfg := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "check", Argv: []string{"/usr/bin/true"}, Parser: "exit-code", Required: true, Timeout: "2s"}}}
	b, _ := json.Marshal(cfg)
	repo, s := testutil.Repo(t, map[string]string{".rover/config.json": string(b), "a": "x"})
	svc, e := New(context.Background(), s, repo, "HEAD", assurance.Options{Mode: "local-advisory", AllowLocal: true}, true)
	if e != nil {
		t.Fatal(e)
	}
	names := map[string]bool{}
	for _, n := range AllNames() {
		names[n] = true
	}
	for _, t := range svc.Tools(nil) {
		names[t.Name] = true
	}
	forbidden := []string{"review", "approve", "grant", "sign", "publish", "merge", "credential", "policy", "config", "delete", "uninstall", "remove", "purge"}
	for name := range names {
		for _, f := range forbidden {
			if strings.Contains(strings.ToLower(name), f) {
				t.Fatalf("tool name %q reaches the %q authority surface", name, f)
			}
		}
	}
}

// TestHostileTaskArgsRejected (A17) proves untrusted tool input cannot change
// executor trust options: unknown keys in rover_task_run are rejected outright.
func TestHostileTaskArgsRejected(t *testing.T) {
	cfg := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "check", Argv: []string{"/usr/bin/true"}, Parser: "exit-code", Required: true, Timeout: "2s"}}}
	b, _ := json.Marshal(cfg)
	repo, s := testutil.Repo(t, map[string]string{".rover/config.json": string(b), "a": "x"})
	svc, e := New(context.Background(), s, repo, "HEAD", assurance.Options{Mode: "local-advisory", AllowLocal: true}, true)
	if e != nil {
		t.Fatal(e)
	}
	args, _ := json.Marshal(map[string]any{
		"objective":    "x",
		"mode":         "restricted-docker",
		"allow_local":  true,
		"docker_image": "example@sha256:" + strings.Repeat("a", 64),
		"pass_env":     []string{"HOME"},
	})
	if _, e = svc.Call(context.Background(), "rover_task_run", args, nil); e == nil {
		t.Fatal("hostile executor-selection keys accepted")
	} else if !strings.Contains(e.Error(), "unknown field") {
		t.Fatalf("rejection did not refuse the hostile keys: %v", e)
	}
}
