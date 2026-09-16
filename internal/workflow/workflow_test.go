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
