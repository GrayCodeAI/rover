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

// TestUnknownProfileRejected covers A03/A04: an undeclared adapter must be an
// explicit error, not silently downgraded to the generic headless profile.
func TestUnknownProfileRejected(t *testing.T) {
	s := model.TaskSpec{Agent: "never-declared"}
	if _, e := Argv(s, "do task", false); e == nil || !strings.Contains(e.Error(), "unknown agent adapter") {
		t.Fatalf("want explicit unknown-adapter error, got %v", e)
	}
	// Name() must not guess a native adapter for an undeclared profile.
	if n := Name(s); n != "never-declared" {
		t.Fatalf("Name() rewrote undeclared adapter to %q", n)
	}
}

// TestMalformedNativeEventRejected covers A03/A04 malformed JSONL handling:
// a corrupt line inside an otherwise native transcript must fail the attempt
// rather than silently becoming a successful run.
func TestMalformedNativeEventRejected(t *testing.T) {
	good := []byte("{\"type\":\"result\",\"session_id\":\"s\",\"is_error\":false,\"result\":\"ok\"}\n")
	cases := [][]byte{
		[]byte("not-json\n"),
		[]byte("{\"result\":\"no type\"}\n"),
		append([]byte("{\"type\":\"result\",\"session_id\":\"s\",\"is_error\":false,\"result\":\"ok\"}\n"), []byte("garbage\n")...),
		append(good, []byte("{\"type\":\"system\"}\n")...),
	}
	for i, b := range cases {
		if _, e := Parse("claude-print", b); e == nil {
			t.Fatalf("case %d: malformed transcript accepted", i)
		}
	}
	// A second terminal event after a completed run must not be silently accepted.
	both := append(append([]byte{}, good...), []byte("{\"type\":\"result\",\"session_id\":\"s\",\"is_error\":false}\n")...)
	if _, e := Parse("claude-print", both); e == nil {
		t.Fatal("event-after-terminal accepted")
	}
}

func TestNativeOptionValidation(t *testing.T) {
	for _, bad := range []string{"-evil", "../evil", "a\x00b", ""} {
		if bad == "" {
			continue // empty means default binary, valid
		}
		s := model.TaskSpec{Agent: "codex-exec", AgentOptions: model.AgentOptions{Executable: bad}}
		if _, e := Argv(s, "task", false); e == nil {
			t.Fatalf("codex executable %q accepted", bad)
		}
		s = model.TaskSpec{Agent: "claude-print", AgentOptions: model.AgentOptions{Executable: bad}}
		if _, e := Argv(s, "task", false); e == nil {
			t.Fatalf("claude executable %q accepted", bad)
		}
	}
	for _, bad := range []string{"--evil", "model with spaces", "m,evil", ""} {
		if bad == "" {
			continue
		}
		s := model.TaskSpec{Agent: "codex-exec", AgentOptions: model.AgentOptions{Model: bad}}
		if _, e := Argv(s, "task", false); e == nil {
			t.Fatalf("model %q accepted", bad)
		}
	}
	s := model.TaskSpec{Agent: "claude-print", AgentOptions: model.AgentOptions{AllowedTools: []string{"--evil"}}}
	if _, e := Argv(s, "task", false); e == nil {
		t.Fatal("evil allowed tool accepted")
	}
}
