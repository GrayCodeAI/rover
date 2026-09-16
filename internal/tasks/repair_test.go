package tasks

import (
	"context"
	"encoding/json"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/testutil"
	"testing"
)

func TestBoundedRepairPreservesAttempts(t *testing.T) {
	cfg := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "value", Argv: []string{"/bin/sh", "-c", `test "$(cat value)" = good`}, Timeout: "2s", Required: true, Parser: "exit-code"}}, Policy: model.Policy{RequireReview: true}}
	b, _ := json.Marshal(cfg)
	repo, s := testutil.Repo(t, map[string]string{"value": "bad", ".rover/config.json": string(b)})
	sp := model.TaskSpec{Schema: model.Schema, Repository: repo, Base: "HEAD", Objective: "fix value", Argv: []string{"/bin/sh", "-c", "printf bad > value"}, RepairArgv: []string{"/bin/sh", "-c", "printf good > value"}, Timeout: "10s", AutoVerify: true, MaxAttempts: 2}
	r, e := Submit(context.Background(), s, sp, "repair", true, true)
	if e != nil {
		t.Fatal(e)
	}
	if r.Status != "REVIEW_READY" || len(r.Attempts) != 2 {
		t.Fatalf("%+v", r)
	}
	var first, last model.Investigation
	if e = s.Get("investigation", r.Attempts[0].Investigation, &first); e != nil {
		t.Fatal(e)
	}
	if e = s.Get("investigation", r.Attempts[1].Investigation, &last); e != nil {
		t.Fatal(e)
	}
	if first.Decision != "BLOCKED" || last.Decision != "REVIEW_REQUIRED" || r.Attempts[0].FeedbackSHA256 == "" {
		t.Fatalf("%+v %+v", first, last)
	}
}
func TestRepairBudgetStops(t *testing.T) {
	cfg := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "fail", Argv: []string{"/bin/sh", "-c", "exit 1"}, Timeout: "1s", Required: true, Parser: "exit-code"}}}
	b, _ := json.Marshal(cfg)
	repo, s := testutil.Repo(t, map[string]string{".rover/config.json": string(b)})
	sp := model.TaskSpec{Schema: model.Schema, Repository: repo, Base: "HEAD", Objective: "bounded", Argv: []string{"/usr/bin/true"}, RepairArgv: []string{"/usr/bin/true"}, Timeout: "10s", AutoVerify: true, MaxAttempts: 3}
	r, e := Submit(context.Background(), s, sp, "", true, true)
	if e != nil || r.Status != "CHECKS_BLOCKED" || len(r.Attempts) != 3 {
		t.Fatalf("%+v %v", r, e)
	}
}
