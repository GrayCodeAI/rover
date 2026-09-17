// Package workflow executes approved bounded DAGs. Integration is deliberately
// file-conservative; it never silently merges contradictory edits or publishes.
package workflow

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/GrayCodeAI/rover/internal/assurance"
	"github.com/GrayCodeAI/rover/internal/config"
	"github.com/GrayCodeAI/rover/internal/execution"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/store"
	"github.com/GrayCodeAI/rover/internal/tasks"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"
)

type Node struct {
	ID        string         `json:"id"`
	Task      model.TaskSpec `json:"task"`
	DependsOn []string       `json:"depends_on,omitempty"`
}
type Spec struct {
	Schema                  string `json:"schema"`
	Objective               string `json:"objective"`
	Repository              string `json:"repository"`
	Base                    string `json:"base"`
	Timeout                 string `json:"timeout"`
	MaxParallel             int    `json:"max_parallel"`
	AllowUnreviewedHandoffs bool   `json:"allow_unreviewed_handoffs"`
	ConfigPath              string `json:"config_path,omitempty"`
	Nodes                   []Node `json:"nodes"`
}
type NodeState struct {
	ID        string `json:"id"`
	TaskID    string `json:"task_id,omitempty"`
	Status    string `json:"status"`
	Candidate string `json:"candidate,omitempty"`
	Input     string `json:"input,omitempty"`
	Error     string `json:"error,omitempty"`
}
type Run struct {
	Schema          string               `json:"schema"`
	ID              string               `json:"id"`
	Spec            Spec                 `json:"spec"`
	Digest          string               `json:"digest"`
	BaseSnapshot    string               `json:"base_snapshot"`
	Config          model.Config         `json:"config"`
	PolicySource    string               `json:"policy_source"`
	Status          string               `json:"status"`
	Nodes           map[string]NodeState `json:"nodes"`
	Candidate       string               `json:"candidate,omitempty"`
	Investigation   string               `json:"investigation,omitempty"`
	CreatedAt       string               `json:"created_at"`
	UpdatedAt       string               `json:"updated_at"`
	CancelRequested bool                 `json:"cancel_requested"`
	PID             int                  `json:"pid"`
	ProcessIdentity string               `json:"process_identity"`
	Error           string               `json:"error,omitempty"`
}

func Validate(s Spec) error {
	if s.Schema != model.Schema || strings.TrimSpace(s.Objective) == "" || len(s.Objective) > 32768 || s.Repository == "" || s.Base == "" {
		return errors.New("workflow schema, objective, repository and base required")
	}
	if len(s.Nodes) < 1 || len(s.Nodes) > 128 || s.MaxParallel < 1 || s.MaxParallel > 32 {
		return errors.New("workflow requires 1..128 nodes and parallelism 1..32")
	}
	d, e := time.ParseDuration(s.Timeout)
	if e != nil || d < time.Millisecond || d > 24*time.Hour {
		return errors.New("workflow timeout must be <=24h")
	}
	nodes := map[string]Node{}
	for _, n := range s.Nodes {
		if !model.ValidID(n.ID) || len(n.ID) > 96 {
			return errors.New("invalid node id")
		}
		if _, ok := nodes[n.ID]; ok {
			return errors.New("duplicate node")
		}
		t := n.Task
		if t.Repository != "" && t.Repository != s.Repository {
			return errors.New("node repository must inherit workflow repository")
		}
		if t.Base != "" && t.Base != s.Base {
			return errors.New("node base must inherit workflow base")
		}
		t.Schema = model.Schema
		t.Repository = s.Repository
		t.Base = s.Base
		if t.ConfigPath != "" || t.InitialSnapshot != "" {
			return errors.New("node cannot replace workflow policy or input snapshot")
		}
		if !t.AutoVerify {
			return errors.New("workflow nodes require auto_verify")
		}
		if e = config.ValidateTask(t); e != nil {
			return fmt.Errorf("node %s: %w", n.ID, e)
		}
		nodes[n.ID] = n
	}
	colors := map[string]int{}
	var visit func(string) error
	visit = func(id string) error {
		if colors[id] == 1 {
			return errors.New("dependency cycle")
		}
		if colors[id] == 2 {
			return nil
		}
		n, ok := nodes[id]
		if !ok {
			return fmt.Errorf("unknown dependency %s", id)
		}
		colors[id] = 1
		seen := map[string]bool{}
		for _, d := range n.DependsOn {
			if seen[d] {
				return errors.New("duplicate dependency")
			}
			seen[d] = true
			if e := visit(d); e != nil {
				return e
			}
		}
		colors[id] = 2
		return nil
	}
	for id := range nodes {
		if e = visit(id); e != nil {
			return e
		}
	}
	return nil
}
func Get(s *store.Store, id string) (r Run, e error) { e = s.Get("workflow", id, &r); return }
func Save(s *store.Store, r *Run, event string) error {
	r.UpdatedAt = model.Now()
	return s.Mutate("workflow", r.ID, event, func(b json.RawMessage) (any, error) {
		if b != nil {
			var old Run
			if e := json.Unmarshal(b, &old); e != nil {
				return nil, e
			}
			if old.CancelRequested {
				r.CancelRequested = true
			}
		}
		return *r, nil
	})
}
func Submit(ctx context.Context, s *store.Store, spec Spec, key string, allow, foreground bool) (Run, error) {
	if !allow {
		return Run{}, errors.New("workflow execution requires --allow-local; not a sandbox")
	}
	if e := Validate(spec); e != nil {
		return Run{}, e
	}
	root, e := source.Discover(ctx, spec.Repository)
	if e != nil {
		return Run{}, e
	}
	if store.IsWithin(root, s.Root) || store.IsWithin(s.Root, root) {
		return Run{}, errors.New("state/repository overlap")
	}
	spec.Repository = root
	base, e := source.Capture(ctx, s, root, spec.Base, false)
	if e != nil {
		return Run{}, e
	}
	cfg, ps, e := assurance.LoadConfig(s, base, spec.ConfigPath)
	if e != nil {
		return Run{}, e
	}
	if len(cfg.Checks) == 0 {
		return Run{}, errors.New("workflow acceptance plan empty")
	}
	spec.Base = base.Commit
	for i := range spec.Nodes {
		spec.Nodes[i].Task.Repository = ""
		spec.Nodes[i].Task.Base = ""
		spec.Nodes[i].Task.Schema = model.Schema
	}
	id := model.ID("flow")
	r := Run{Schema: model.Schema, ID: id, Spec: spec, BaseSnapshot: base.ID, Config: cfg, PolicySource: ps, Status: "QUEUED", Nodes: map[string]NodeState{}, CreatedAt: model.Now(), UpdatedAt: model.Now()}
	for _, n := range spec.Nodes {
		r.Nodes[n.ID] = NodeState{ID: n.ID, Status: "PENDING"}
	}
	r.Digest = model.Hash(struct {
		S Spec
		C model.Config
	}{spec, cfg})
	k := ""
	if key != "" {
		k = "workflow:" + key
	}
	actual, exists, e := s.CreateOnce("workflow", id, k, r.Digest, r)
	if e != nil {
		return r, e
	}
	if exists {
		return Get(s, actual)
	}
	if foreground {
		e = Execute(ctx, s, id)
		rr, re := Get(s, id)
		if e != nil {
			return rr, e
		}
		return rr, re
	}
	dir := filepath.Join(s.Root, "workflows", id)
	if e = store.PrivateDir(dir); e != nil {
		return r, e
	}
	exe, e := os.Executable()
	if e != nil {
		return r, e
	}
	cmd := exec.Command(exe, "--state", s.Root, "__workflow", "--id", id)
	cmd.Dir = dir
	env := []string{"PATH=" + os.Getenv("PATH"), "GOTOOLCHAIN=local"}
	seen := map[string]bool{}
	for _, n := range spec.Nodes {
		for _, k := range n.Task.PassEnv {
			if !seen[k] {
				if v, ok := os.LookupEnv(k); ok {
					env = append(env, k+"="+v)
				}
				seen[k] = true
			}
		}
	}
	for _, ch := range cfg.Checks {
		for _, k := range ch.PassEnv {
			if !seen[k] {
				if v, ok := os.LookupEnv(k); ok {
					env = append(env, k+"="+v)
				}
				seen[k] = true
			}
		}
	}
	cmd.Env = env
	log, e := os.OpenFile(filepath.Join(dir, "supervisor.log"), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if e != nil {
		return r, e
	}
	defer log.Close()
	cmd.Stdout = log
	cmd.Stderr = log
	execution.Detach(cmd)
	if e = cmd.Start(); e != nil {
		r.Status = "ERROR"
		r.Error = e.Error()
		_ = Save(s, &r, "workflow.launch_error")
		return r, e
	}
	pid := cmd.Process.Pid
	if e := cmd.Process.Release(); e != nil {
		r.Status = "ERROR"
		r.Error = e.Error()
		_ = Save(s, &r, "workflow.launch_error")
		return r, e
	}
	_ = s.Mutate("workflow", id, "workflow.launched", func(b json.RawMessage) (any, error) {
		var x Run
		if e := json.Unmarshal(b, &x); e != nil {
			return nil, e
		}
		if x.PID == 0 {
			x.PID = pid
			x.ProcessIdentity = tasks.ProcessIdentity(pid)
		}
		return x, nil
	})
	return Get(s, id)
}
func Execute(parent context.Context, s *store.Store, id string) (ret error) {
	r, e := Get(s, id)
	if e != nil {
		return e
	}
	if e = Validate(r.Spec); e != nil {
		return e
	}
	if r.Digest != model.Hash(struct {
		S Spec
		C model.Config
	}{r.Spec, r.Config}) {
		return errors.New("workflow contract identity mismatch")
	}
	e = s.Mutate("workflow", id, "workflow.started", func(b json.RawMessage) (any, error) {
		if e := json.Unmarshal(b, &r); e != nil {
			return nil, e
		}
		if r.Status != "QUEUED" {
			return nil, errors.New("workflow already claimed; no automatic replay")
		}
		r.Status = "RUNNING"
		r.PID = os.Getpid()
		r.ProcessIdentity = tasks.ProcessIdentity(os.Getpid())
		return r, nil
	})
	if e != nil {
		return e
	}
	defer func() {
		if ret != nil {
			r.Status = "ERROR"
			r.Error = ret.Error()
			if errors.Is(ret, context.Canceled) || errors.Is(ret, context.DeadlineExceeded) {
				r.Status = "CANCELLED"
			}
			_ = Save(s, &r, "workflow.error")
		}
	}()
	base, e := source.Load(s, r.BaseSnapshot)
	if e != nil {
		return e
	}
	dir := filepath.Join(s.Root, "workflows", id)
	if e = store.PrivateDir(dir); e != nil {
		return e
	}
	cfgpath := filepath.Join(dir, "approved-config.json")
	cb, _ := json.Marshal(r.Config)
	if e = store.AtomicFile(cfgpath, cb, 0600); e != nil {
		return e
	}
	duration, _ := time.ParseDuration(r.Spec.Timeout)
	created, e := time.Parse(time.RFC3339Nano, r.CreatedAt)
	if e != nil {
		return e
	}
	ctx, cancel := context.WithDeadline(parent, created.Add(duration))
	defer cancel()
	monitor := make(chan struct{})
	go func() {
		defer close(monitor)
		tick := time.NewTicker(100 * time.Millisecond)
		defer tick.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-tick.C:
				v, e := Get(s, id)
				if e != nil || v.CancelRequested {
					cancel()
					return
				}
			}
		}
	}()
	defer func() { cancel(); <-monitor }()
	active := map[string]bool{}
	var wg sync.WaitGroup
	defer func() { cancel(); wg.Wait() }()
	finish := func(status string, err error) error {
		r.Status = status
		if err != nil {
			r.Error = err.Error()
		}
		return Save(s, &r, "workflow.finished")
	}
	ids := make([]string, 0, len(r.Nodes))
	by := map[string]Node{}
	for _, n := range r.Spec.Nodes {
		ids = append(ids, n.ID)
		by[n.ID] = n
	}
	sort.Strings(ids)
	for {
		if ctx.Err() != nil {
			for n := range active {
				_ = tasks.Cancel(s, r.Nodes[n].TaskID)
			}
			wg.Wait()
			return finish("CANCELLED", ctx.Err())
		}
		for _, n := range ids {
			ns := r.Nodes[n]
			if ns.TaskID == "" || (ns.Status != "RUNNING" && ns.Status != "AWAITING_REVIEW") {
				continue
			}
			t, e := tasks.Get(s, ns.TaskID)
			if e != nil {
				return e
			}
			if !model.Terminal(t.Status) {
				continue
			}
			if t.Status != "REVIEW_READY" {
				delete(active, n)
				ns.Status = "FAILED"
				ns.Error = t.Status + ": " + t.Error
				r.Nodes[n] = ns
				continue
			}
			var in model.Investigation
			if e = s.Get("investigation", t.InvestigationID, &in); e != nil {
				return e
			}
			if in.Decision != "ACCEPTED" && in.Decision != "REVIEW_REQUIRED" {
				delete(active, n)
				ns.Status = "FAILED"
				ns.Error = "candidate lacks required verification"
				r.Nodes[n] = ns
				continue
			}
			if in.Decision == "REVIEW_REQUIRED" && !r.Spec.AllowUnreviewedHandoffs {
				reviewed, e := hasReview(s, in)
				if e != nil {
					return e
				}
				if !reviewed {
					delete(active, n)
					ns.Status = "AWAITING_REVIEW"
					r.Nodes[n] = ns
					continue
				}
			}
			delete(active, n)
			ns.Status = "VERIFIED"
			ns.Candidate = t.Candidate
			r.Nodes[n] = ns
		}
		all, failed := true, false
		for _, ns := range r.Nodes {
			switch ns.Status {
			case "FAILED", "SKIPPED":
				failed = true
			case "VERIFIED":
			default:
				all = false
			}
		}
		if all {
			if failed {
				return finish("FAILED", errors.New("dependency work failed"))
			}
			break
		}
		for _, n := range ids {
			if len(active) >= r.Spec.MaxParallel {
				break
			}
			ns := r.Nodes[n]
			if ns.Status != "PENDING" {
				continue
			}
			ready, bad := true, false
			parents := []model.Snapshot{}
			for _, d := range by[n].DependsOn {
				ds := r.Nodes[d]
				if ds.Status == "FAILED" || ds.Status == "SKIPPED" {
					bad = true
				}
				if ds.Status != "VERIFIED" {
					ready = false
				} else {
					x, e := source.Load(s, ds.Candidate)
					if e != nil {
						return e
					}
					parents = append(parents, x)
				}
			}
			if bad {
				ns.Status = "SKIPPED"
				ns.Error = "dependency failed"
				r.Nodes[n] = ns
				continue
			}
			if !ready {
				continue
			}
			input := base
			if len(parents) > 0 {
				input, e = source.Merge(s, base, parents)
				if e != nil {
					ns.Status = "FAILED"
					ns.Error = e.Error()
					r.Nodes[n] = ns
					continue
				}
			}
			ts := by[n].Task
			ts.Schema = model.Schema
			ts.Repository = r.Spec.Repository
			ts.Base = base.Commit
			ts.ConfigPath = cfgpath
			ts.InitialSnapshot = input.ID
			tr, e := tasks.Prepare(ctx, s, ts, id+"."+n, true)
			if e != nil {
				ns.Status = "FAILED"
				ns.Error = e.Error()
				r.Nodes[n] = ns
				continue
			}
			ns.TaskID = tr.ID
			ns.Input = input.ID
			ns.Status = "RUNNING"
			r.Nodes[n] = ns
			if e = Save(s, &r, "workflow.node_admitted"); e != nil {
				return e
			}
			active[n] = true
			wg.Add(1)
			go func(id string) { defer wg.Done(); _ = tasks.Execute(ctx, s, id) }(tr.ID)
		}
		if e = Save(s, &r, "workflow.progress"); e != nil {
			return e
		}
		select {
		case <-ctx.Done():
		case <-time.After(100 * time.Millisecond):
		}
	}
	wg.Wait()
	if ctx.Err() != nil {
		return finish("CANCELLED", ctx.Err())
	}
	r.Status = "INTEGRATING"
	if e = Save(s, &r, "workflow.integrating"); e != nil {
		return e
	}
	used := map[string]bool{}
	for _, n := range r.Spec.Nodes {
		for _, d := range n.DependsOn {
			used[d] = true
		}
	}
	leaves := []model.Snapshot{}
	for _, id := range ids {
		if !used[id] {
			x, e := source.Load(s, r.Nodes[id].Candidate)
			if e != nil {
				return e
			}
			leaves = append(leaves, x)
		}
	}
	merged, e := source.Merge(s, base, leaves)
	if e != nil {
		return finish("CONFLICT", e)
	}
	r.Candidate = merged.ID
	in, e := assurance.Verify(ctx, s, base, merged, r.Config, assurance.Options{Mode: "local-advisory", AllowLocal: true, PolicySource: r.PolicySource})
	if e != nil {
		return e
	}
	r.Investigation = in.ID
	if ctx.Err() != nil {
		return finish("CANCELLED", ctx.Err())
	}
	if in.Decision == "ACCEPTED" || in.Decision == "REVIEW_REQUIRED" {
		return finish("REVIEW_READY", nil)
	}
	return finish("FAILED", errors.New("integrated candidate did not satisfy required checks"))
}
func hasReview(s *store.Store, in model.Investigation) (bool, error) {
	rows, e := s.ListAll("approval", 100000)
	if e != nil {
		return false, e
	}
	for _, b := range rows {
		var r model.Approval
		if json.Unmarshal(b, &r) == nil && r.InvestigationID == in.ID && r.Candidate == in.Candidate && r.ConfigDigest == in.ConfigDigest {
			return true, nil
		}
	}
	return false, nil
}
func Cancel(s *store.Store, id string) error {
	return s.Mutate("workflow", id, "workflow.cancel_requested", func(b json.RawMessage) (any, error) {
		if b == nil {
			return nil, store.ErrNotFound
		}
		var r Run
		if e := json.Unmarshal(b, &r); e != nil {
			return nil, e
		}
		r.CancelRequested = true
		return r, nil
	})
}

// Reconcile reports lost supervisors; it does not recreate external side effects.
func Reconcile(s *store.Store) error {
	rows, e := s.ListAll("workflow", 100000)
	if e != nil {
		return e
	}
	for _, b := range rows {
		var r Run
		if e = json.Unmarshal(b, &r); e != nil {
			return e
		}
		switch r.Status {
		case "QUEUED", "RUNNING", "INTEGRATING":
		default:
			continue
		}
		t, e := time.Parse(time.RFC3339Nano, r.CreatedAt)
		if e != nil {
			return e
		}
		if r.PID == 0 && time.Since(t) < 10*time.Second {
			continue
		}
		if !tasks.DefinitelyGone(r.PID, r.ProcessIdentity) {
			continue
		}
		r.Status = "LOST"
		r.Error = "supervisor not alive; explicit new workflow required, no implicit replay"
		if e = Save(s, &r, "workflow.lost"); e != nil {
			return e
		}
	}
	return nil
}
