package tasks

import (
	"context"
	"encoding/json"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/testutil"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestForegroundTaskAndIdempotency(t *testing.T) {
	cfg := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "test", Argv: []string{"sh", "-c", `test "$(cat value)" = fixed`}, Timeout: "2s", Required: true, Parser: "exit-code"}}}
	b, _ := json.Marshal(cfg)
	repo, s := testutil.Repo(t, map[string]string{"value": "broken", ".rover/config.json": string(b)})
	spec := model.TaskSpec{Schema: model.Schema, Objective: "fix fixture", Repository: repo, Base: "HEAD", Argv: []string{"sh", "-c", "printf fixed > value; echo completed"}, Timeout: "10s", AutoVerify: true}
	run, e := Submit(context.Background(), s, spec, "once", true, true)
	if e != nil {
		t.Fatal(e)
	}
	if run.Status != "REVIEW_READY" {
		t.Fatalf("%+v", run)
	}
	original, _ := os.ReadFile(filepath.Join(repo, "value"))
	if string(original) != "broken" {
		t.Fatal("main checkout modified")
	}
	again, e := Submit(context.Background(), s, spec, "once", true, true)
	if e != nil || again.ID != run.ID {
		t.Fatal(e, again)
	}
	items, _ := s.List("task", 100)
	if len(items) != 1 {
		t.Fatal("duplicate run")
	}
	var in model.Investigation
	if e = s.Get("investigation", run.InvestigationID, &in); e != nil || in.Candidate != run.Candidate {
		t.Fatal(e)
	}
}
func TestTaskCancellation(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	spec := model.TaskSpec{Schema: model.Schema, Objective: "cancel fixture", Repository: repo, Base: "HEAD", Argv: []string{"sh", "-c", "echo started; sleep 20"}, Timeout: "30s"}
	done := make(chan error, 1)
	go func() { _, e := Submit(context.Background(), s, spec, "cancel", true, true); done <- e }()
	deadline := time.Now().Add(5 * time.Second)
	var id string
	for time.Now().Before(deadline) {
		records, e := s.List("task", 10)
		if e != nil {
			t.Fatal(e)
		}
		if len(records) > 0 {
			var r model.TaskRun
			json.Unmarshal(records[0], &r)
			if r.Status == "RUNNING" {
				id = r.ID
				break
			}
		}
		time.Sleep(20 * time.Millisecond)
	}
	if id == "" {
		t.Fatal("task did not start")
	}
	if e := Cancel(s, id); e != nil {
		t.Fatal(e)
	}
	select {
	case e := <-done:
		if e != nil {
			t.Fatal(e)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("cancel did not terminate task")
	}
	r, e := Get(s, id)
	if e != nil || r.Status != "CANCELLED" {
		t.Fatal(e, r)
	}
}
func TestLostWorkerReconciliation(t *testing.T) {
	_, s := testutil.Repo(t, nil)
	r := model.TaskRun{ID: "task_lost", Status: "RUNNING", PID: 1000000000, Heartbeat: time.Now().Add(-time.Hour).UTC().Format(time.RFC3339Nano)}
	s.Put("task", r.ID, r, "")
	if e := Reconcile(s); e != nil {
		t.Fatal(e)
	}
	r, e := Get(s, r.ID)
	if e != nil || r.Status != "LOST" {
		t.Fatal(e, r)
	}
}
func TestCapabilitiesAreHonest(t *testing.T) {
	c := Capabilities()
	if !c.Launch || !c.LogFollow || c.InteractivePTY || c.NativeResume || c.PermissionMediation || c.UsageReporting {
		t.Fatal(c)
	}
}

func TestQueuedLaunchedProcessCanBeReconciled(t *testing.T) {
	_, s := testutil.Repo(t, nil)
	r := model.TaskRun{ID: "task_failed_start", Status: "QUEUED", PID: 1000000000, Heartbeat: time.Now().Add(-time.Hour).UTC().Format(time.RFC3339Nano)}
	if e := s.Put("task", r.ID, r, ""); e != nil {
		t.Fatal(e)
	}
	if e := Reconcile(s); e != nil {
		t.Fatal(e)
	}
	got, e := Get(s, r.ID)
	if e != nil || got.Status != "LOST" {
		t.Fatal(e, got)
	}
}
