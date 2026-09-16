package cli

import (
	"bytes"
	"context"
	"encoding/json"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/testutil"
	"os"
	"path/filepath"
	"testing"
)

func TestInitIsNonDestructive(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"AGENTS.md": "keep me", "go.mod": "module example.invalid/fixture\n\ngo 1.23\n"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	args := []string{"--state", s.Root, "init", "--repo", repo, "--json"}
	if code := app.Main(context.Background(), args); code != 0 {
		t.Fatal(code, err.String(), out.String())
	}
	if _, e := os.Stat(filepath.Join(repo, ".rover/config.json")); !os.IsNotExist(e) {
		t.Fatal("preview mutated files")
	}
	args = append(args, "--apply")
	out.Reset()
	if code := app.Main(context.Background(), args); code != 0 {
		t.Fatal(code, err.String())
	}
	if code := app.Main(context.Background(), args); code != 2 {
		t.Fatal("overwrote config")
	}
	b, _ := os.ReadFile(filepath.Join(repo, "AGENTS.md"))
	if string(b) != "keep me" {
		t.Fatal("agent instructions changed")
	}
}
func TestJSONErrorAndExitCodes(t *testing.T) {
	var out, err bytes.Buffer
	a := New(&out, &err)
	code := a.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), "no-such-command", "--json"})
	if code != 2 || !json.Valid(out.Bytes()) {
		t.Fatal(code, out.String())
	}
	for _, tc := range []struct {
		s    string
		code int
	}{{"ACCEPTED", 0}, {"BLOCKED", 1}, {"INCONCLUSIVE", 2}, {"REVIEW_REQUIRED", 3}, {"PENDING", 2}} {
		if DecisionExit(tc.s) != tc.code {
			t.Fatal(tc)
		}
	}
}
func TestCLIRejectsExecutionWithoutGrant(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	a := New(&out, &err)
	code := a.Main(context.Background(), []string{"--state", s.Root, "verify", "--repo", repo, "--json"})
	if code != 2 || !json.Valid(out.Bytes()) {
		t.Fatal(code, out.String())
	}
}
func TestReviewDoesNotRewriteDecision(t *testing.T) {
	_, s := testutil.Repo(t, nil)
	in := model.Investigation{ID: "inv_one", Candidate: "snapshot_one", ConfigDigest: "digest", Decision: "REVIEW_REQUIRED"}
	s.Put("investigation", in.ID, in, "")
	var out, err bytes.Buffer
	a := New(&out, &err)
	code := a.Main(context.Background(), []string{"--state", s.Root, "review", "--id", in.ID, "--note", "reviewed locally", "--json"})
	if code != 0 {
		t.Fatal(code, out.String())
	}
	s.Get("investigation", in.ID, &in)
	if in.Decision != "REVIEW_REQUIRED" {
		t.Fatal("approval fabricated a new verdict")
	}
}

func TestExplicitStateDoesNotRequireHome(t *testing.T) {
	t.Setenv("HOME", "")
	t.Setenv("XDG_CONFIG_HOME", "")
	t.Setenv("ROVER_HOME", "")
	var out, errs bytes.Buffer
	app := New(&out, &errs)
	if rc := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), "status", "--json"}); rc != 0 {
		t.Fatal(rc, out.String(), errs.String())
	}
}
