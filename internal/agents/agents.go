// Package agents adapts documented headless CLIs. Provider messages are claims,
// not verification evidence. Live-account conformance is reported separately.
package agents

import (
	"bufio"
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/GrayCodeAI/rover/internal/model"
	"strings"
)

type Descriptor struct {
	Name                string `json:"name"`
	Executable          string `json:"executable"`
	Structured          bool   `json:"structured"`
	Interactive         bool   `json:"interactive"`
	PermissionMediation bool   `json:"permission_mediation"`
	Validation          string `json:"validation"`
}

func List() []Descriptor {
	return []Descriptor{
		{"generic-headless", "user-defined", false, false, false, "local command integration"},
		{"generic-pty", "user-defined", false, true, false, "Linux PTY; local advisory only"},
		{"codex-exec", "codex", true, false, false, "documented CLI; fixture conformance; live-account testing required"},
		{"claude-print", "claude", true, false, false, "documented CLI; fixture conformance; live-account testing required"},
	}
}
func Name(s model.TaskSpec) string {
	if s.Agent != "" {
		return s.Agent
	}
	if s.Interactive {
		return "generic-pty"
	}
	return "generic-headless"
}
func Argv(s model.TaskSpec, prompt string, repair bool) ([]string, error) {
	switch Name(s) {
	case "generic-headless", "generic-pty":
		a := s.Argv
		if repair {
			a = s.RepairArgv
		}
		if len(a) == 0 {
			return nil, errors.New("executable required")
		}
		out := append([]string(nil), a...)
		for i, x := range out {
			if x == "{{objective}}" {
				out[i] = prompt
			}
		}
		return out, nil
	case "codex-exec":
		e := s.AgentOptions.Executable
		if e == "" {
			e = "codex"
		}
		sandbox := "read-only"
		if s.AgentOptions.Write {
			sandbox = "workspace-write"
		}
		a := []string{e, "exec", "--json", "--sandbox", sandbox}
		if s.AgentOptions.Model != "" {
			a = append(a, "--model", s.AgentOptions.Model)
		}
		return append(a, "--", prompt), nil
	case "claude-print":
		e := s.AgentOptions.Executable
		if e == "" {
			e = "claude"
		}
		mode := "plan"
		if s.AgentOptions.Write {
			mode = "acceptEdits"
		}
		a := []string{e, "--print", "--output-format", "stream-json", "--verbose", "--permission-mode", mode}
		if s.AgentOptions.MaxTurns > 0 {
			a = append(a, "--max-turns", fmt.Sprint(s.AgentOptions.MaxTurns))
		}
		if s.AgentOptions.Model != "" {
			a = append(a, "--model", s.AgentOptions.Model)
		}
		if len(s.AgentOptions.AllowedTools) > 0 {
			a = append(a, "--allowedTools", strings.Join(s.AgentOptions.AllowedTools, ","))
		}
		return append(a, "--", prompt), nil
	default:
		return nil, errors.New("unknown agent adapter")
	}
}
func Parse(adapter string, b []byte) (model.AgentResult, error) {
	r := model.AgentResult{Adapter: adapter, Usage: map[string]int64{}, Provenance: "provider-emitted claims/usage; not independent verification"}
	if adapter != "codex-exec" && adapter != "claude-print" {
		return r, nil
	}
	if len(b) > 1<<20 {
		return r, errors.New("transcript exceeds limit")
	}
	sc := bufio.NewScanner(bytes.NewReader(b))
	sc.Buffer(make([]byte, 4096), 1<<20)
	terminal := false
	seen := map[string]string{}
	for sc.Scan() {
		if len(bytes.TrimSpace(sc.Bytes())) == 0 {
			continue
		}
		var e map[string]json.RawMessage
		if json.Unmarshal(sc.Bytes(), &e) != nil {
			return r, errors.New("malformed native event")
		}
		str := func(k string) string { var v string; _ = json.Unmarshal(e[k], &v); return v }
		kind := str("type")
		if kind == "" {
			return r, errors.New("event missing type")
		}
		if id := str("uuid"); id != "" {
			h := model.Digest(sc.Bytes())
			if old, ok := seen[id]; ok {
				if old != h {
					return r, errors.New("conflicting duplicate event")
				}
				continue
			}
			seen[id] = h
		}
		if terminal {
			return r, errors.New("event after terminal result")
		}
		if adapter == "codex-exec" {
			switch kind {
			case "thread.started":
				v := str("thread_id")
				if v == "" || (r.Session != "" && r.Session != v) {
					return r, errors.New("invalid thread identity")
				}
				r.Session = v
			case "turn.started", "item.started", "item.updated":
			case "item.completed":
				var item struct{ Type, Text string }
				if json.Unmarshal(e["item"], &item) != nil {
					return r, errors.New("invalid item")
				}
				if item.Type == "agent_message" {
					r.Claims = append(r.Claims, item.Text)
				}
			case "turn.completed":
				if r.Session == "" {
					return r, errors.New("completion without thread")
				}
				terminal = true
				r.Completed = true
				readUsage(e["usage"], &r)
			case "turn.failed", "error":
				terminal = true
				r.Error = string(e["error"])
				if r.Error == "" {
					r.Error = "provider error"
				}
			default:
				r.UnknownEvents++
			}
		} else {
			if id := str("session_id"); id != "" {
				if r.Session != "" && r.Session != id {
					return r, errors.New("session changed")
				}
				r.Session = id
			}
			switch kind {
			case "system", "assistant", "user", "stream_event", "tool_progress", "tool_use_summary":
			case "result":
				var failed *bool
				if json.Unmarshal(e["is_error"], &failed) != nil || failed == nil || r.Session == "" {
					return r, errors.New("terminal result requires session and is_error boolean")
				}
				terminal = true
				r.Completed = !*failed
				if *failed {
					r.Error = str("result")
					if r.Error == "" {
						r.Error = "provider failed"
					}
				}
				if x := str("result"); x != "" {
					r.Claims = append(r.Claims, x)
				}
				readUsage(e["usage"], &r)
				var cost *float64
				if json.Unmarshal(e["total_cost_usd"], &cost) == nil && cost != nil && *cost >= 0 {
					r.EstimatedCostUSD = cost
				}
			default:
				r.UnknownEvents++
			}
		}
		if len(r.Claims) > 256 {
			return r, errors.New("claim capture limit exceeded")
		}
	}
	if e := sc.Err(); e != nil {
		return r, e
	}
	if !terminal {
		return r, errors.New("terminal result missing")
	}
	return r, nil
}
func readUsage(b []byte, r *model.AgentResult) {
	var raw map[string]json.RawMessage
	if json.Unmarshal(b, &raw) != nil {
		return
	}
	for k, v := range raw {
		var n int64
		if json.Unmarshal(v, &n) == nil && n >= 0 {
			r.Usage[k] = n
		}
	}
}
