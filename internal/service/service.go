// Package service is the project-bound application surface used by MCP and HTTP.
// It deliberately excludes review, grants, signatures, policy changes and merge.
package service

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/GrayCodeAI/rover/internal/access"
	"github.com/GrayCodeAI/rover/internal/agents"
	"github.com/GrayCodeAI/rover/internal/assurance"
	"github.com/GrayCodeAI/rover/internal/contextstore"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/store"
	"github.com/GrayCodeAI/rover/internal/tasks"
	"github.com/GrayCodeAI/rover/internal/wire"
	"path/filepath"
	"time"
)

type Service struct {
	Store           *store.Store
	Repository      string
	Base            model.Snapshot
	Config          model.Config
	PolicySource    string
	Options         assurance.Options
	EnableExecution bool
	PassEnv         []string
}
type Tool struct {
	Name        string         `json:"name"`
	Description string         `json:"description"`
	InputSchema map[string]any `json:"inputSchema"`
	Annotations map[string]any `json:"annotations"`
}

func New(ctx context.Context, s *store.Store, repo, base string, o assurance.Options, enable bool) (*Service, error) {
	root, e := source.Discover(ctx, repo)
	if e != nil {
		return nil, e
	}
	snap, e := source.Capture(ctx, s, root, base, false)
	if e != nil {
		return nil, e
	}
	cfg, ps, e := assurance.LoadConfig(s, snap, "")
	if e != nil {
		return nil, fmt.Errorf("server requires an approved committed base config: %w", e)
	}
	o.PolicySource = ps
	return &Service{Store: s, Repository: root, Base: snap, Config: cfg, PolicySource: ps, Options: o, EnableExecution: enable}, nil
}
func schema(props map[string]any, required ...string) map[string]any {
	m := map[string]any{"type": "object", "properties": props, "additionalProperties": false}
	if len(required) > 0 {
		m["required"] = required
	}
	return m
}
func field(t string) map[string]any { return map[string]any{"type": t} }
func (s *Service) Tools(g *access.Grant) []Tool {
	tools := []Tool{
		{"rover_agent_capabilities", "List actual adapter declarations; no live-account certification is implied.", schema(map[string]any{}), nil},
		{"rover_inspect", "Capture this project's candidate and inspect against the server-pinned approved base.", schema(map[string]any{"working_tree": field("boolean"), "include_untracked": field("boolean")}), nil},
		{"rover_status", "Read only tasks in this project; output explicitly reports truncation.", schema(map[string]any{"task_id": field("string")}), nil},
		{"rover_report", "Read this project's snapshot-bound evidence, not a universal correctness score.", schema(map[string]any{"investigation_id": field("string")}, "investigation_id"), nil},
		{"rover_diff", "Return this project's applicable candidate patch.", schema(map[string]any{"investigation_id": field("string")}, "investigation_id"), nil},
		{"rover_context_search", "Bounded literal search in an identified project snapshot.", schema(map[string]any{"snapshot_id": field("string"), "query": field("string"), "limit": field("integer")}, "query"), nil},
	}
	if s.EnableExecution {
		tools = append(tools, Tool{"rover_verify", "Execute the server-approved checks against an exact project candidate. Tool completion does not imply acceptance.", schema(map[string]any{"candidate_id": field("string")}, "candidate_id"), nil}, Tool{"rover_task_run", "Submit approved-local headless work; result is submission, not task acceptance. Requires a powerful execution grant.", schema(map[string]any{"objective": field("string"), "agent": field("string"), "argv": map[string]any{"type": "array", "items": field("string")}, "write": field("boolean"), "timeout": field("string"), "key": field("string")}, "objective"), nil}, Tool{"rover_task_cancel", "Request cancellation of this project's task; external side effects are not undone.", schema(map[string]any{"task_id": field("string")}, "task_id"), nil})
	}
	out := []Tool{}
	for _, t := range tools {
		readTool := t.Name != "rover_verify" && t.Name != "rover_task_run" && t.Name != "rover_task_cancel"
		if g != nil && !g.Allows(t.Name) {
			continue
		}
		t.Annotations = map[string]any{"readOnlyHint": readTool, "destructiveHint": !readTool, "openWorldHint": !readTool}
		out = append(out, t)
	}
	return out
}
func isExecutionTool(name string) bool {
	return name == "rover_verify" || name == "rover_task_run" || name == "rover_task_cancel"
}
func AllNames() []string {
	return []string{"rover_agent_capabilities", "rover_inspect", "rover_status", "rover_report", "rover_diff", "rover_context_search", "rover_verify", "rover_task_run", "rover_task_cancel"}
}
func (s *Service) Call(ctx context.Context, name string, b json.RawMessage, g *access.Grant) (any, error) {
	// Re-validate a network grant against current store state so revocation
	// or expiry between Authenticate and Call is not bypassed. A nil grant
	// remains local stdio UID-trust.
	if g != nil {
		var fresh access.Grant
		if e := s.Store.Get("grant", g.ID, &fresh); e != nil {
			return nil, access.ErrDenied
		}
		exp, e := time.Parse(time.RFC3339Nano, fresh.ExpiresAt)
		if e != nil || fresh.Revoked || !time.Now().Before(exp) {
			return nil, access.ErrDenied
		}
		if fresh.Audience != access.Audience(s.Store, s.Repository) || fresh.Project != access.ProjectID(s.Repository) {
			return nil, access.ErrDenied
		}
		g = &fresh
	}
	allowed := false
	for _, t := range s.Tools(g) {
		if t.Name == name {
			allowed = true
		}
	}
	if !allowed {
		return nil, access.ErrDenied
	}
	// Local stdio (nil grant) may not execute; validate args then deny so
	// hostile executor-selection keys surface as field errors, not silent
	// downgrades. Network callers must carry a valid grant for any tool.
	if g == nil && isExecutionTool(name) {
		switch name {
		case "rover_task_run":
			var a struct {
				Objective string   `json:"objective"`
				Agent     string   `json:"agent"`
				Argv      []string `json:"argv"`
				Write     bool     `json:"write"`
				Timeout   string   `json:"timeout"`
				Key       string   `json:"key"`
			}
			if e := wire.Decode(b, &a); e != nil {
				return nil, e
			}
		case "rover_task_cancel":
			var a struct {
				ID string `json:"task_id"`
			}
			if e := wire.Decode(b, &a); e != nil {
				return nil, e
			}
		case "rover_verify":
			var a struct {
				Candidate string `json:"candidate_id"`
			}
			if e := wire.Decode(b, &a); e != nil {
				return nil, e
			}
		}
		return nil, access.ErrDenied
	}
	if len(b) == 0 {
		b = []byte("{}")
	}
	switch name {
	case "rover_agent_capabilities":
		var a struct{}
		if e := wire.Decode(b, &a); e != nil {
			return nil, e
		}
		return map[string]any{"schema": model.Schema, "adapters": agents.List()}, nil
	case "rover_inspect":
		var a struct {
			WorkingTree      bool `json:"working_tree"`
			IncludeUntracked bool `json:"include_untracked"`
		}
		if e := wire.Decode(b, &a); e != nil {
			return nil, e
		}
		ref := "HEAD"
		if a.WorkingTree {
			ref = "WORKTREE"
		}
		cand, e := source.Capture(ctx, s.Store, s.Repository, ref, a.IncludeUntracked)
		if e != nil {
			return nil, e
		}
		return source.Compare(s.Base, cand), nil
	case "rover_status":
		var a struct {
			TaskID string `json:"task_id"`
		}
		if e := wire.Decode(b, &a); e != nil {
			return nil, e
		}
		if a.TaskID != "" {
			if !model.ValidID(a.TaskID) {
				return nil, access.ErrDenied
			}
			return s.task(a.TaskID)
		}
		raw, e := s.Store.ListAll("task", 100000)
		if e != nil {
			return nil, e
		}
		out := []model.TaskRun{}
		more := false
		for _, b := range raw {
			var t model.TaskRun
			if e = json.Unmarshal(b, &t); e != nil {
				return nil, e
			}
			if t.Contract.Repository == s.Repository {
				if len(out) >= 200 {
					more = true
					continue
				}
				out = append(out, t)
			}
		}
		return map[string]any{"tasks": out, "truncated": more}, nil
	case "rover_report", "rover_diff":
		var a struct {
			ID string `json:"investigation_id"`
		}
		if e := wire.Decode(b, &a); e != nil {
			return nil, e
		}
		if !model.ValidID(a.ID) {
			return nil, access.ErrDenied
		}
		in, e := s.investigation(a.ID)
		if e != nil {
			return nil, e
		}
		if name == "rover_report" {
			return in, nil
		}
		base, e := s.snapshot(in.Base)
		if e != nil {
			return nil, e
		}
		cand, e := s.snapshot(in.Candidate)
		if e != nil {
			return nil, e
		}
		patch, e := source.Diff(ctx, s.Store, base, cand)
		if e != nil {
			return nil, e
		}
		return map[string]any{"candidate": cand.ID, "patch": string(patch)}, nil
	case "rover_context_search":
		var a struct {
			Snapshot string `json:"snapshot_id"`
			Query    string `json:"query"`
			Limit    int    `json:"limit"`
		}
		if e := wire.Decode(b, &a); e != nil {
			return nil, e
		}
		var snap model.Snapshot
		var e error
		if a.Snapshot != "" {
			if !model.ValidID(a.Snapshot) {
				return nil, access.ErrDenied
			}
			snap, e = s.snapshot(a.Snapshot)
		} else {
			snap, e = source.Capture(ctx, s.Store, s.Repository, "HEAD", false)
		}
		if e != nil {
			return nil, e
		}
		if a.Limit == 0 {
			a.Limit = 30
		}
		return contextstore.Search(ctx, s.Store, snap, a.Query, a.Limit)
	case "rover_verify":
		var a struct {
			Candidate string `json:"candidate_id"`
		}
		if e := wire.Decode(b, &a); e != nil {
			return nil, e
		}
		if !model.ValidID(a.Candidate) {
			return nil, access.ErrDenied
		}
		cand, e := s.snapshot(a.Candidate)
		if e != nil {
			return nil, e
		}
		return assurance.Verify(ctx, s.Store, s.Base, cand, s.Config, s.Options)
	case "rover_task_cancel":
		var a struct {
			ID string `json:"task_id"`
		}
		if e := wire.Decode(b, &a); e != nil {
			return nil, e
		}
		if !model.ValidID(a.ID) {
			return nil, access.ErrDenied
		}
		if _, e := s.task(a.ID); e != nil {
			return nil, e
		}
		if e := tasks.Cancel(s.Store, a.ID); e != nil {
			return nil, e
		}
		return s.task(a.ID)
	case "rover_task_run":
		if !s.Options.AllowLocal || s.Options.Mode != "local-advisory" {
			return nil, errors.New("agent task launch only available in explicitly enabled local-advisory service mode")
		}
		var a struct {
			Objective string   `json:"objective"`
			Agent     string   `json:"agent"`
			Argv      []string `json:"argv"`
			Write     bool     `json:"write"`
			Timeout   string   `json:"timeout"`
			Key       string   `json:"key"`
		}
		if e := wire.Decode(b, &a); e != nil {
			return nil, e
		}
		if a.Agent == "generic-pty" {
			return nil, errors.New("MCP/API task creation is headless")
		}
		if a.Timeout == "" {
			a.Timeout = "30m"
		}
		if d, e := time.ParseDuration(a.Timeout); e != nil || d <= 0 || d > 24*time.Hour {
			return nil, errors.New("invalid task timeout")
		}
		cb, _ := json.Marshal(s.Config)
		dir := filepath.Join(s.Store.Root, "api", model.Digest([]byte(s.Repository))[:24])
		if e := store.PrivateDir(dir); e != nil {
			return nil, e
		}
		file := filepath.Join(dir, "config-"+model.Hash(s.Config)+".json")
		if e := store.AtomicFile(file, cb, 0600); e != nil {
			return nil, e
		}
		spec := model.TaskSpec{Schema: model.Schema, Objective: a.Objective, Repository: s.Repository, Base: s.Base.Commit, Argv: a.Argv, Agent: a.Agent, AgentOptions: model.AgentOptions{Write: a.Write}, Timeout: a.Timeout, AutoVerify: true, ConfigPath: file, PassEnv: s.PassEnv, MaxAttempts: 1}
		if spec.Argv == nil {
			spec.Argv = []string{}
		}
		key := a.Key
		if key != "" {
			key = "api." + model.Digest([]byte(s.Repository+"\x00"+key))
		}
		return tasks.Submit(ctx, s.Store, spec, key, true, false)
	}
	return nil, errors.New("unsupported tool")
}
func (s *Service) snapshot(id string) (model.Snapshot, error) {
	x, e := source.Load(s.Store, id)
	if e != nil || x.Repository != s.Repository {
		return model.Snapshot{}, access.ErrDenied
	}
	return x, nil
}
func (s *Service) task(id string) (model.TaskRun, error) {
	x, e := tasks.Get(s.Store, id)
	if e != nil || x.Contract.Repository != s.Repository {
		return model.TaskRun{}, access.ErrDenied
	}
	return x, nil
}
func (s *Service) investigation(id string) (model.Investigation, error) {
	var x model.Investigation
	if e := s.Store.Get("investigation", id, &x); e != nil {
		return x, access.ErrDenied
	}
	if x.Repository != s.Repository {
		return model.Investigation{}, access.ErrDenied
	}
	if _, e := s.snapshot(x.Candidate); e != nil {
		return model.Investigation{}, e
	}
	return x, nil
}
