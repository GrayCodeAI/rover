package workflow

import (
	"context"
	"encoding/json"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/testutil"
	"testing"
)

func node(id, command string, deps ...string) Node {
	return Node{ID: id, DependsOn: deps, Task: model.TaskSpec{Objective: id, Argv: []string{"/bin/sh", "-c", command}, Timeout: "10s", AutoVerify: true}}
}
func TestCycleRejected(t *testing.T) {
	s := Spec{Schema: model.Schema, Objective: "cycle", Repository: "/repo", Base: "HEAD", Timeout: "10s", MaxParallel: 2, Nodes: []Node{node("a", "true", "b"), node("b", "true", "a")}}
	if Validate(s) == nil {
		t.Fatal("cycle accepted")
	}
}
func TestParallelDependencyAndIntegration(t *testing.T) {
	cfg := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "check", Argv: []string{"/usr/bin/true"}, Timeout: "2s", Required: true, Parser: "exit-code"}}, Policy: model.Policy{RequireReview: true}}
	b, _ := json.Marshal(cfg)
	repo, s := testutil.Repo(t, map[string]string{"README.md": "fixture", ".rover/config.json": string(b)})
	spec := Spec{Schema: model.Schema, Objective: "parallel feature", Repository: repo, Base: "HEAD", Timeout: "20s", MaxParallel: 2, AllowUnreviewedHandoffs: true, Nodes: []Node{node("a", "printf a > a.txt"), node("b", "printf b > b.txt"), node("join", "test -f a.txt && test -f b.txt && printf joined > combined.txt", "a", "b")}}
	r, e := Submit(context.Background(), s, spec, "dag", true, true)
	if e != nil {
		t.Fatal(e)
	}
	if r.Status != "REVIEW_READY" {
		t.Fatalf("%+v", r)
	}
	cand, e := source.Load(s, r.Candidate)
	if e != nil {
		t.Fatal(e)
	}
	for _, p := range []string{"a.txt", "b.txt", "combined.txt"} {
		if _, e = source.FileContent(s, cand, p); e != nil {
			t.Fatal(p, e)
		}
	}
	if r.Investigation == "" {
		t.Fatal("integration not verified")
	}
	again, e := Submit(context.Background(), s, spec, "dag", true, true)
	if e != nil || again.ID != r.ID {
		t.Fatal("idempotency", again.ID, e)
	}
}
func TestIntegrationConflict(t *testing.T) {
	cfg := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "check", Argv: []string{"/usr/bin/true"}, Timeout: "2s", Required: true, Parser: "exit-code"}}}
	b, _ := json.Marshal(cfg)
	repo, s := testutil.Repo(t, map[string]string{"same": "base", ".rover/config.json": string(b)})
	spec := Spec{Schema: model.Schema, Objective: "conflict", Repository: repo, Base: "HEAD", Timeout: "20s", MaxParallel: 2, AllowUnreviewedHandoffs: true, Nodes: []Node{node("a", "printf a > same"), node("b", "printf b > same")}}
	r, e := Submit(context.Background(), s, spec, "", true, true)
	if e != nil || r.Status != "CONFLICT" {
		t.Fatalf("%+v %v", r, e)
	}
}

// TestFrozenConfigBinding covers A14/A15: the run pins the config that was
// loaded at submit time, and an idempotency key may not be reused under a
// different config contract.
func TestFrozenConfigBinding(t *testing.T) {
	cfg1 := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "one", Argv: []string{"/usr/bin/true"}, Timeout: "2s", Required: true, Parser: "exit-code"}}}
	cfg2 := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "two", Argv: []string{"/usr/bin/true"}, Timeout: "2s", Required: true, Parser: "exit-code"}}}
	b1, _ := json.Marshal(cfg1)
	repo, s := testutil.Repo(t, map[string]string{"value": "base", ".rover/config.json": string(b1)})
	spec := Spec{Schema: model.Schema, Objective: "frozen", Repository: repo, Base: "HEAD", Timeout: "20s", MaxParallel: 1, AllowUnreviewedHandoffs: true, Nodes: []Node{node("a", "true")}}
	r1, e := Submit(context.Background(), s, spec, "k-frozen", true, true)
	if e != nil {
		t.Fatal(e)
	}
	if r1.Config.Checks[0].ID != "one" {
		t.Fatalf("expected frozen config 'one', got %q", r1.Config.Checks[0].ID)
	}
	again, e := Submit(context.Background(), s, spec, "k-frozen", true, true)
	if e != nil || again.ID != r1.ID {
		t.Fatalf("same key same contract must dedupe: %v %v", again.ID, e)
	}
	// A different plan (committed, since the plan loads from the base snapshot)
	// under the same idempotency key is a contract reuse.
	b2, _ := json.Marshal(cfg2)
	testutil.Write(t, repo, ".rover/config.json", string(b2))
	testutil.Commit(t, repo)
	if _, e = Submit(context.Background(), s, spec, "k-frozen", true, true); e == nil {
		t.Fatal("idempotency key reused with a different contract accepted")
	}
}
