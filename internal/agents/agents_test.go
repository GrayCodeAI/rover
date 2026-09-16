package agents

import (
	"github.com/GrayCodeAI/rover/internal/model"
	"strings"
	"testing"
)

func TestProfiles(t *testing.T) {
	s := model.TaskSpec{Agent: "codex-exec"}
	a, e := Argv(s, "do task", false)
	if e != nil || !strings.Contains(strings.Join(a, " "), "--sandbox read-only") {
		t.Fatalf("%v %v", a, e)
	}
	s.AgentOptions.Write = true
	a, _ = Argv(s, "task", false)
	if !strings.Contains(strings.Join(a, " "), "workspace-write") {
		t.Fatal(a)
	}
	s.Agent = "claude-print"
	a, _ = Argv(s, "task", false)
	if strings.Contains(strings.Join(a, " "), "bypass") {
		t.Fatal(a)
	}
}
func TestCodexTranscript(t *testing.T) {
	b := []byte("{\"type\":\"thread.started\",\"thread_id\":\"t1\"}\n{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"all correct\"}}\n{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":8}}\n")
	r, e := Parse("codex-exec", b)
	if e != nil || !r.Completed || r.Session != "t1" || len(r.Claims) != 1 || r.Usage["input_tokens"] != 8 {
		t.Fatalf("%+v %v", r, e)
	}
	for _, b := range []string{`{}`, `{"type":"turn.completed"}`, `{"type":"thread.started","thread_id":"t1"}`} {
		if _, e := Parse("codex-exec", []byte(b)); e == nil {
			t.Fatalf("accepted %s", b)
		}
	}
}
func TestClaudeTranscript(t *testing.T) {
	r, e := Parse("claude-print", []byte(`{"type":"result","session_id":"x","is_error":false,"result":"fixed","total_cost_usd":0.04}`))
	if e != nil || !r.Completed || len(r.Claims) != 1 || r.EstimatedCostUSD == nil {
		t.Fatalf("%+v %v", r, e)
	}
	for _, b := range []string{`{"type":"result","session_id":"x","is_error":null}`, `{"type":"result","is_error":false}`} {
		if _, e := Parse("claude-print", []byte(b)); e == nil {
			t.Fatal("invalid terminal accepted")
		}
	}
}
func FuzzTranscript(f *testing.F) {
	f.Add([]byte(`{"type":"result"}`))
	f.Add([]byte(`null`))
	f.Fuzz(func(t *testing.T, b []byte) { _, _ = Parse("claude-print", b); _, _ = Parse("codex-exec", b) })
}
