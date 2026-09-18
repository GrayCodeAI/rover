package cli

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	model "github.com/GrayCodeAI/rover/internal/model"
	testutil "github.com/GrayCodeAI/rover/internal/testutil"
)

func TestInitPreviewDoesNotMutate(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{})
	var out, err bytes.Buffer
	app := New(&out, &err)
	args := []string{"--state", s.Root, "init", "--repo", repo, "--json"}
	if code := app.Main(context.Background(), args); code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if _, e := os.Stat(filepath.Join(repo, ".rover/config.json")); !os.IsNotExist(e) {
		t.Fatal("preview mutated files")
	}
	args = append(args, "--apply")
	out.Reset()
	if code := app.Main(context.Background(), args); code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if code := app.Main(context.Background(), args); code != 2 {
		t.Fatal("overwrote config")
	}
}

func TestJSONErrorsRenderValidJSON(t *testing.T) {
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), "no-such-command", "--json"})
	if code != 2 || !json.Valid(out.Bytes()) {
		t.Fatal(code, out.String(), err.String())
	}
	for _, tc := range []struct {
		s    string
		code int
	}{{"ACCEPTED", 0}, {"BLOCKED", 1}, {"INCONCLUSIVE", 2}, {"REVIEW_REQUIRED", 3}, {"PENDING", 2}} {
		if got := DecisionExit(tc.s); got != tc.code {
			t.Fatal(tc.s, got)
		}
	}
}

func TestNoInstallerOrUninstallerCommand(t *testing.T) {
	for _, cmd := range []string{"install", "uninstall", "remove", "purge"} {
		var out, err bytes.Buffer
		app := New(&out, &err)
		if code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), cmd, "--json"}); code != 2 {
			t.Fatalf("%s not rejected", cmd)
		}
	}
}

func TestLogsAndReportRenderStripHostileBytes(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	const id = "hostile_one"
	hostile := []byte("clean\x1b[31mred\x00nul\r\ncar\x9bCSIrest")
	dir := filepath.Join(s.Root, "tasks", id, "output")
	if e := os.MkdirAll(dir, 0o755); e != nil {
		t.Fatal(e)
	}
	if e := os.WriteFile(filepath.Join(dir, "stdout.log"), hostile, 0o644); e != nil {
		t.Fatal(e)
	}
	hostileMeaning := "meaning with\x1b[31m color\x00 and\r\n newlines\x9b end"
	inv := model.Investigation{ID: id, Candidate: "snapshot_one", ConfigDigest: "d", Decision: "INCONCLUSIVE",
		Checks: []model.CheckResult{{ID: "c1", Outcome: "INCONCLUSIVE", Meaning: hostileMeaning}}}
	if e := s.Put("investigation", inv.ID, inv, ""); e != nil {
		t.Fatal(e)
	}
	run := model.TaskRun{ID: id, Status: "COMPLETED", InvestigationID: inv.ID}
	if e := s.Put("task", run.ID, run, ""); e != nil {
		t.Fatal(e)
	}
	{
		var out, err bytes.Buffer
		app := New(&out, &err)
		if code := app.Main(context.Background(), []string{"--state", s.Root, "logs", "--id", id}); code != 0 {
			t.Fatal(code, out.String(), err.String())
		}
		for _, b := range []byte{0x1b, 0x00, 0x0d} {
			if bytes.IndexByte(out.Bytes(), b) >= 0 {
				t.Fatalf("hostile byte %#x leaked into logs render: %x", b, out.Bytes())
			}
		}
		if !bytes.Contains(out.Bytes(), []byte("clean")) || !bytes.Contains(out.Bytes(), []byte("car")) {
			t.Fatalf("sanitized logs render lost trace content: %q", out.Bytes())
		}
	}
	{
		var out, err bytes.Buffer
		app := New(&out, &err)
		if code := app.Main(context.Background(), []string{"--state", s.Root, "logs", "--id", id, "--json"}); code != 0 {
			t.Fatal(code, out.String(), err.String())
		}
		if !json.Valid(out.Bytes()) {
			t.Fatal("logs json render invalid", out.String())
		}
	}
	{
		var out, err bytes.Buffer
		app := New(&out, &err)
		if code := app.Main(context.Background(), []string{"--state", s.Root, "report", "--id", id}); code != 0 {
			t.Fatal(code, out.String(), err.String())
		}
		for _, b := range []byte{0x1b, 0x00, 0x0d} {
			if bytes.IndexByte(out.Bytes(), b) >= 0 {
				t.Fatalf("hostile byte %#x leaked into report render: %x", b, out.Bytes())
			}
		}
	}
	{
		var out, err bytes.Buffer
		app := New(&out, &err)
		if code := app.Main(context.Background(), []string{"--state", s.Root, "report", "--id", id, "--json"}); code != 0 {
			t.Fatal(code, out.String(), err.String())
		}
		if !json.Valid(out.Bytes()) {
			t.Fatal("report json render invalid", out.String())
		}
	}
}

func TestShortAliasesRoute(t *testing.T) {
	// Aliases must reach the real command, not "unknown command".
	cases := []struct {
		args []string
		want string
	}{
		{[]string{"lg", "--id", "bad id"}, "valid task ID required"},
		{[]string{"rep", "--id", "bad id"}, "valid investigation ID required"},
		{[]string{"st", "--id", "bad id"}, "valid task ID required"},
	}
	for _, c := range cases {
		var out, err bytes.Buffer
		app := New(&out, &err)
		full := append([]string{"--state", filepath.Join(t.TempDir(), "state")}, c.args...)
		code := app.Main(context.Background(), full)
		if code != 2 {
			t.Fatalf("%v: code=%d", c.args, code)
		}
		got := out.String() + err.String()
		if !bytes.Contains([]byte(got), []byte(c.want)) {
			t.Fatalf("%v: want %q in %q", c.args, c.want, got)
		}
		if bytes.Contains([]byte(got), []byte("unknown command")) {
			t.Fatalf("%v: alias not routed", c.args)
		}
	}
	// wf must reach workflow routing, not top-level unknown-command.
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), "wf", "bogus", "--json"})
	if code != 2 {
		t.Fatal(code)
	}
	if bytes.Contains(out.Bytes(), []byte("unknown command")) {
		t.Fatalf("wf alias not routed: %s", out.String())
	}
}

func TestCheckRequiresAdmissionLikeVerify(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	for _, cmd := range []string{"check", "verify"} {
		var out, err bytes.Buffer
		app := New(&out, &err)
		code := app.Main(context.Background(), []string{"--state", s.Root, cmd, "--repo", repo, "--json"})
		if code != 2 {
			t.Fatalf("%s without --allow-local: code=%d", cmd, code)
		}
		if !bytes.Contains(out.Bytes(), []byte("allow-local")) {
			t.Fatalf("%s without --allow-local: %s", cmd, out.String())
		}
	}
}

func TestDoReadsTaskFile(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "do", "--file", filepath.Join(t.TempDir(), "missing.json"), "--allow-local", "--json"})
	if code != 2 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("do error is not valid JSON", out.String())
	}
}

func TestVersionCommand(t *testing.T) {
	var out, err bytes.Buffer
	app := New(&out, &err)
	state := filepath.Join(t.TempDir(), "state")
	if code := app.Main(context.Background(), []string{"--state", state, "version", "--json"}); code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	var v map[string]any
	if !json.Valid(out.Bytes()) {
		t.Fatal("version output is not valid JSON", out.String())
	}
	if e := json.Unmarshal(out.Bytes(), &v); e != nil {
		t.Fatal(e)
	}
	if v["name"] != "Rover" {
		t.Errorf("name = %v, want Rover", v["name"])
	}
	if v["schema"] == nil {
		t.Error("schema field missing")
	}
	if v["go"] == nil {
		t.Error("go field missing")
	}
	if v["sqlite"] == nil {
		t.Error("sqlite field missing")
	}
}

func TestDoctorCommand(t *testing.T) {
	var out, err bytes.Buffer
	app := New(&out, &err)
	state := filepath.Join(t.TempDir(), "state")
	if code := app.Main(context.Background(), []string{"--state", state, "doctor", "--json"}); code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("doctor output is not valid JSON", out.String())
	}
	var d map[string]any
	if e := json.Unmarshal(out.Bytes(), &d); e != nil {
		t.Fatal(e)
	}
	if d["tools"] == nil {
		t.Error("tools field missing")
	}
	if d["trust_modes"] == nil {
		t.Error("trust_modes field missing")
	}
}

func TestAgentsCommand(t *testing.T) {
	var out, err bytes.Buffer
	app := New(&out, &err)
	state := filepath.Join(t.TempDir(), "state")
	if code := app.Main(context.Background(), []string{"--state", state, "agents", "--json"}); code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("agents output is not valid JSON", out.String())
	}
	var adapters []any
	if e := json.Unmarshal(out.Bytes(), &adapters); e != nil {
		t.Fatal("agents output should be a JSON array", e, out.String())
	}
	if len(adapters) == 0 {
		t.Error("expected non-empty adapters list")
	}
}

func TestHelpAndNoArgs(t *testing.T) {
	var out, err bytes.Buffer
	app := New(&out, &err)
	state := filepath.Join(t.TempDir(), "state")
	// No args → help text
	if code := app.Main(context.Background(), []string{"--state", state}); code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !bytes.Contains(out.Bytes(), []byte("Usage:")) {
		t.Fatal("no-args should print help text")
	}
	// Explicit help
	out.Reset()
	if code := app.Main(context.Background(), []string{"--state", state, "help"}); code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !bytes.Contains(out.Bytes(), []byte("Usage:")) {
		t.Fatal("help command should print help text")
	}
}

func TestUnknownCommandReturnsExit2(t *testing.T) {
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), "nonexistent-cmd", "--json"})
	if code != 2 {
		t.Fatalf("unknown command: expected exit 2, got %d", code)
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("unknown command --json should emit valid JSON", out.String())
	}
	if !bytes.Contains(out.Bytes(), []byte("nonexistent-cmd")) {
		t.Fatal("unknown command name missing from error")
	}
}

func TestUnknownFlagReturnsExit2(t *testing.T) {
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), "--bogus-flag", "--json"})
	if code != 2 {
		t.Fatalf("unknown flag: expected exit 2, got %d", code)
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("unknown flag --json should emit valid JSON", out.String())
	}
}

func TestEventsInvalidID(t *testing.T) {
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), "events", "--id", "bad id", "--json"})
	if code != 2 {
		t.Fatalf("events invalid id: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("valid record ID")) {
		t.Fatal("expected valid record ID error")
	}
}

func TestEventsMissingID(t *testing.T) {
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), "events", "--json"})
	if code != 2 {
		t.Fatalf("events missing id: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("valid record ID")) {
		t.Fatal("expected valid record ID error for missing --id")
	}
}

func TestExportInvalidKind(t *testing.T) {
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), "export", "--kind", "bogus", "--json"})
	if code != 2 {
		t.Fatalf("export invalid kind: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("unsupported export kind")) {
		t.Fatal("expected unsupported export kind error")
	}
}

func TestExportValidKind(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "export", "--kind", "task", "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("export output is not valid JSON", out.String())
	}
	var r map[string]any
	if !json.Valid(out.Bytes()) {
		t.Fatal("export output is not valid JSON", out.String())
	}
	if e := json.Unmarshal(out.Bytes(), &r); e != nil {
		t.Fatal(e)
	}
	if r["kind"] != "task" {
		t.Errorf("kind = %v, want task", r["kind"])
	}
}

func TestPublishMissingFlags(t *testing.T) {
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), "publish", "--json"})
	if code != 2 {
		t.Fatalf("publish missing flags: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("publish requires")) {
		t.Fatal("expected publish requires --id and --to error")
	}
}

func TestCancelInvalidID(t *testing.T) {
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), "cancel", "--id", "bad id", "--json"})
	if code != 2 {
		t.Fatalf("cancel invalid id: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("valid task ID")) {
		t.Fatal("expected valid task ID error")
	}
}

func TestTUIRequiresTerminal(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "tui", "--json"})
	if code != 2 {
		t.Fatalf("tui without TTY: expected exit 2, got %d", code)
	}
	if !bytes.Contains(err.Bytes(), []byte("Interactive TUI requires")) {
		t.Fatal("expected TUI terminal requirement message")
	}
}

func TestExtendedCommandsWithoutSubcommand(t *testing.T) {
	state := filepath.Join(t.TempDir(), "state")
	cases := []struct {
		cmd string
		err string
	}{
		{"attest", "attest keygen|sign|verify"},
		{"learn", "learn recommend|dataset|evaluate|promote|show|revoke"},
		{"grant", "grant create|list|revoke"},
		{"remote", "remote tools|call|node-add|node-list|node-delete"},
		{"context", "context search|bundle, memory put|list|delete"},
		{"memory", "context search|bundle, memory put|list|delete"},
	}
	for _, tc := range cases {
		var out, err bytes.Buffer
		app := New(&out, &err)
		code := app.Main(context.Background(), []string{"--state", state, tc.cmd, "--json"})
		if code != 2 {
			t.Errorf("%s without subcommand: expected exit 2, got %d (out=%q, err=%q)", tc.cmd, code, out.String(), err.String())
			continue
		}
		if !bytes.Contains(out.Bytes(), []byte(tc.err)) {
			t.Errorf("%s without subcommand: expected %q in output, got %q", tc.cmd, tc.err, out.String())
		}
	}
}

func TestLimitsInvalidMaxAgents(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "limits", "--max-agents", "129", "--json"})
	if code != 2 {
		t.Fatalf("limits --max-agents 129: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("max-agents outside 1..128")) {
		t.Fatal("expected max-agents out of range error")
	}
}

func TestLimitsValidQuery(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "limits", "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("limits output is not valid JSON", out.String())
	}
	var r map[string]any
	if !json.Valid(out.Bytes()) {
		t.Fatal("limits output is not valid JSON", out.String())
	}
	if e := json.Unmarshal(out.Bytes(), &r); e != nil {
		t.Fatal(e)
	}
	if r["max_agents"] == nil {
		t.Error("max_agents field missing from limits output")
	}
}

func TestWorkflowUnknownSubcommand(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "wf", "bogus", "--json"})
	if code != 2 {
		t.Fatalf("workflow bogus: expected exit 2, got %d", code)
	}
	if bytes.Contains(out.Bytes(), []byte("unknown command")) {
		t.Fatal("wf alias should be routed, not unknown command")
	}
	if !bytes.Contains(out.Bytes(), []byte("unknown workflow subcommand")) {
		t.Fatal("expected unknown workflow subcommand error")
	}
}

func TestGrantCreateWithValidTool(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "grant", "create", "--repo", repo, "--tool", "rover_status", "--note", "test", "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("grant create output is not valid JSON", out.String())
	}
	var r map[string]any
	if e := json.Unmarshal(out.Bytes(), &r); e != nil {
		t.Fatal(e)
	}
	if r["grant"] == nil {
		t.Error("grant field missing")
	}
	if r["token"] == nil {
		t.Error("token field missing")
	}
}

func TestGrantCreateUnknownTool(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "grant", "create", "--repo", repo, "--tool", "rover_bogus", "--json"})
	if code != 2 {
		t.Fatalf("grant create unknown tool: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("unknown tool")) {
		t.Fatal("expected unknown tool error")
	}
}

func TestGrantList(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "grant", "list", "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("grant list output is not valid JSON", out.String())
	}
}

func TestAgentCapabilitiesValid(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "agents", "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	var adapters []any
	if !json.Valid(out.Bytes()) {
		t.Fatal("agents output is not valid JSON", out.String())
	}
	if e := json.Unmarshal(out.Bytes(), &adapters); e != nil {
		t.Fatal("agents output should be a JSON array", e, out.String())
	}
	if len(adapters) == 0 {
		t.Fatal("expected non-empty adapters list")
	}
}

func TestAgentCapabilitiesInvalidAdapter(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "agent", "capabilities", "nonexistent", "--json"})
	if code != 2 {
		t.Fatalf("agent capabilities bogus: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("agent list | agent capabilities")) {
		t.Fatal("expected usage error for unknown adapter")
	}
}

func TestRemoteInvalidTimeout(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "remote", "node-list", "--timeout", "0s", "--json"})
	if code != 2 {
		t.Fatalf("remote invalid timeout: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("timeout must be between zero and 24h")) {
		t.Fatal("expected timeout range error")
	}
}

func TestInspectRequiresRepo(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	// inspect does not call execution.Admit (no execution), so it does not
	// require --allow-local. But it needs a valid repo path.
	code := app.Main(context.Background(), []string{"--state", s.Root, "inspect", "--repo", filepath.Join(t.TempDir(), "nonexistent"), "--json"})
	if code != 2 {
		t.Fatalf("inspect nonexistent repo: expected exit 2, got %d", code)
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("inspect error should be valid JSON", out.String())
	}
}

func TestVerifyWithoutAllowLocal(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "verify", "--repo", repo, "--json"})
	if code != 2 {
		t.Fatalf("verify without --allow-local: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("allow-local")) {
		t.Fatal("expected allow-local hint in error")
	}
}

func TestCheckShortcutRequiresAllowLocal(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "check", "--repo", repo, "--json"})
	if code != 2 {
		t.Fatalf("check without --allow-local: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("allow-local")) {
		t.Fatal("expected allow-local hint in error")
	}
}

func TestCheckShortcutWithAllowLocal(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "check", "--repo", repo, "--allow-local", "--json"})
	// 0 = accepted, 1 = blocked, 2 = inconclusive/error. We only assert it's
	// not a flag-parse or routing error.
	if code > 3 {
		t.Fatalf("check shortcut: expected 0-3, got %d", code)
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("check shortcut --json output is not valid JSON", out.String())
	}
}

func TestOutcomeInvalidLabel(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "outcome", "--id", "valid_id", "--label", "INCONCLUSIVE", "--note", "test", "--json"})
	if code != 2 {
		t.Fatalf("outcome invalid label: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("unsupported outcome label")) {
		t.Fatal("expected unsupported outcome label error")
	}
}

func TestOutcomeInvalidID(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "outcome", "--id", "bad id", "--label", "human-accepted", "--note", "test", "--json"})
	if code != 2 {
		t.Fatalf("outcome invalid id: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("valid investigation ID")) {
		t.Fatal("expected valid investigation ID error")
	}
}

func TestOutcomeMissingNote(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "outcome", "--id", "valid_id", "--label", "human-accepted", "--json"})
	if code != 2 {
		t.Fatalf("outcome missing note: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("outcome note required")) {
		t.Fatal("expected outcome note required error")
	}
}

func TestStatusMissingID(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "status", "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("status output is not valid JSON", out.String())
	}
}

func TestLogsMissingID(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "logs", "--json"})
	if code == 0 {
		t.Fatal("logs without --id should not succeed silently")
	}
}

func TestReportMissingID(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "report", "--json"})
	if code != 2 {
		t.Fatalf("report missing id: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("valid investigation ID")) {
		t.Fatal("expected valid investigation ID error")
	}
}

func TestTaskInvalidSubcommand(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "task", "bogus", "--json"})
	if code != 2 {
		t.Fatalf("task bogus: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("usage")) {
		t.Fatal("expected usage error", out.String())
	}
}

func TestJSONErrorRenderingForUnknownCommand(t *testing.T) {
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), "totally-bogus", "--json"})
	if code != 2 {
		t.Fatalf("expected exit 2, got %d", code)
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("unknown command --json should produce valid JSON", out.String())
	}
	if !bytes.Contains(out.Bytes(), []byte("error")) {
		t.Fatal("JSON error should contain error field")
	}
}

func TestStateFlagOverridesDefault(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "init", "--repo", repo, "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("init output is not valid JSON", out.String())
	}
}

func TestAttestKeygenMissingFlags(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "attest", "keygen", "--json"})
	if code != 2 {
		t.Fatalf("attest keygen missing flags: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("distinct new --key and --public-key required")) {
		t.Fatal("expected keygen argument error")
	}
}

func TestAttestKeygenSuccessful(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	dir := t.TempDir()
	dir, _ = filepath.EvalSymlinks(dir)
	keyPath := filepath.Join(dir, "private.key")
	pubPath := filepath.Join(dir, "public.key")
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "attest", "keygen", "--key", keyPath, "--public-key", pubPath, "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("attest keygen output is not valid JSON", out.String())
	}
	var r map[string]any
	if e := json.Unmarshal(out.Bytes(), &r); e != nil {
		t.Fatal(e)
	}
	if r["private_key"] != keyPath {
		t.Errorf("private_key = %v, want %s", r["private_key"], keyPath)
	}
	if _, e := os.Stat(keyPath); e != nil {
		t.Fatal("private key file not created")
	}
	if _, e := os.Stat(pubPath); e != nil {
		t.Fatal("public key file not created")
	}
}

func TestAttestKeygenKeyPathExists(t *testing.T) {
	dir := t.TempDir()
	keyPath := filepath.Join(dir, "exists.key")
	if e := os.MkdirAll(filepath.Dir(keyPath), 0700); e != nil {
		t.Fatal(e)
	}
	if e := os.WriteFile(keyPath, []byte("exists"), 0600); e != nil {
		t.Fatal(e)
	}
	pubPath := filepath.Join(dir, "public.key")
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(dir, "state"), "attest", "keygen", "--key", keyPath, "--public-key", pubPath, "--json"})
	if code != 2 {
		t.Fatalf("expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("already exists")) {
		t.Fatal("expected key path already exists error")
	}
}

func TestAttestVerifyMissingFile(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "attest", "verify", "--public-key", filepath.Join(t.TempDir(), "missing.pub"), "--file", filepath.Join(t.TempDir(), "missing.env"), "--json"})
	if code != 2 {
		t.Fatalf("attest verify missing file: expected exit 2, got %d", code)
	}
}

func TestAttestInvalidSubcommand(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "attest", "bogus", "--json"})
	if code != 2 {
		t.Fatalf("attest bogus: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("attest keygen|sign|verify")) {
		t.Fatal("expected usage error")
	}
}

func TestGrantRevokeValid(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	// First create a grant
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "grant", "create", "--repo", repo, "--tool", "rover_status", "--note", "test grant", "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	var r map[string]any
	if e := json.Unmarshal(out.Bytes(), &r); e != nil {
		t.Fatal(e)
	}
	g := r["grant"].(map[string]any)
	grantID := g["id"].(string)

	// Now revoke it
	out.Reset()
	err.Reset()
	app2 := New(&out, &err)
	code = app2.Main(context.Background(), []string{"--state", s.Root, "grant", "revoke", "--repo", repo, "--id", grantID, "--json"})
	if code != 0 {
		t.Fatal(code, out.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("grant revoke output is not valid JSON", out.String())
	}
	r2 := map[string]any{}
	if e := json.Unmarshal(out.Bytes(), &r2); e != nil {
		t.Fatal(e)
	}
	if r2["status"] != "revoked" {
		t.Errorf("status = %v, want revoked", r2["status"])
	}
}

func TestGrantRevokeWrongID(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "grant", "revoke", "--id", "nonexistentgrant123", "--json"})
	if code != 2 {
		t.Fatalf("grant revoke nonexistent: expected exit 2, got %d", code)
	}
}

func TestRemoteUnknownOperation(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "remote", "bogus", "--json"})
	if code != 2 {
		t.Fatalf("remote bogus: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("unknown remote operation")) {
		t.Fatal("expected unknown remote operation error")
	}
}

func TestRemoteNodeList(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "remote", "node-list", "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("remote node-list output is not valid JSON", out.String())
	}
}

func TestLimitsInvalidMaxAgentsNegative(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "limits", "--max-agents", "-1", "--json"})
	if code != 2 {
		t.Fatalf("limits --max-agents -1: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("max-agents outside 1..128")) {
		t.Fatal("expected max-agents out of range error")
	}
}

func TestLimitsSetMaxAgents(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "limits", "--max-agents", "4", "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("limits output is not valid JSON", out.String())
	}
	var r map[string]any
	if e := json.Unmarshal(out.Bytes(), &r); e != nil {
		t.Fatal(e)
	}
	if r["max_agents"] == nil {
		t.Error("max_agents field missing")
	}
}

func TestStatusWithInvalidID(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "status", "--id", "bad id", "--json"})
	if code != 2 {
		t.Fatalf("status invalid id: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("valid task ID")) {
		t.Fatal("expected valid task ID error")
	}
}

func TestStatusWatchRequiresTerminal(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "status", "--watch"})
	if code != 2 {
		t.Fatalf("status --watch without TTY: expected exit 2, got %d", code)
	}
	combined := out.String() + err.String()
	if !bytes.Contains([]byte(combined), []byte("--watch requires a terminal")) {
		t.Fatal("expected watch requires terminal error", combined)
	}
}

func TestLogsInvalidID(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "logs", "--id", "bad id", "--json"})
	if code != 2 {
		t.Fatalf("logs invalid id: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("valid task ID")) {
		t.Fatal("expected valid task ID error")
	}
}

func TestLogsJSONFollowIncompatible(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "logs", "--id", "valid_id", "--follow", "--json"})
	if code != 2 {
		t.Fatalf("logs --follow --json: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("JSON follow is not supported")) {
		t.Fatal("expected JSON follow incompatible error")
	}
}

func TestDoWithMissingFile(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "do", "--file", filepath.Join(t.TempDir(), "missing.json"), "--allow-local", "--json"})
	if code != 2 {
		t.Fatalf("do missing file: expected exit 2, got %d", code)
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("do error should be valid JSON", out.String())
	}
}

func TestDoWithValidTaskFile(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	taskFile := filepath.Join(t.TempDir(), "task.json")
	taskContent := `{"schema":"rover/v1alpha1","objective":"list files","repository":"` + repo + `","base":"HEAD","argv":["/usr/bin/true"],"timeout":"5s"}`
	if e := os.WriteFile(taskFile, []byte(taskContent), 0600); e != nil {
		t.Fatal(e)
	}
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "do", "--file", taskFile, "--allow-local", "--json"})
	if code > 3 {
		t.Fatalf("do with valid task: expected 0-3, got %d", code)
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("do output should be valid JSON", out.String())
	}
}

func TestTaskRunWithMissingFile(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "task", "run", "--file", filepath.Join(t.TempDir(), "missing.json"), "--allow-local", "--json"})
	if code != 2 {
		t.Fatalf("task run missing file: expected exit 2, got %d", code)
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("task run error should be valid JSON", out.String())
	}
}

func TestTaskRunMissingFile(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "task", "run", "--allow-local", "--json"})
	if code != 2 {
		t.Fatalf("task run --file required: expected exit 2, got %d", code)
	}
}

func TestCheckWithInvalidMode(t *testing.T) {
	repo, _ := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), "check", "--repo", repo, "--allow-local", "--mode", "restricted-docker", "--json"})
	if code != 2 {
		t.Fatalf("check invalid mode (no image): expected exit 2, got %d", code)
	}
}

func TestInspectWithWorktree(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "inspect", "--repo", repo, "--worktree", "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("inspect output is not valid JSON", out.String())
	}
}

func TestInspectWithoutWorktree(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "inspect", "--repo", repo, "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("inspect output is not valid JSON", out.String())
	}
}

func TestCheckWithUntrackedRequiresWorktree(t *testing.T) {
	repo, _ := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), "check", "--repo", repo, "--allow-local", "--include-untracked", "--json"})
	if code != 2 {
		t.Fatalf("check --include-untracked without --worktree: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("--include-untracked requires --worktree")) {
		t.Fatal("expected untracked requires worktree error")
	}
}

func TestStatusWatchAndJSONIncompatible(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "status", "--watch", "--json"})
	if code != 2 {
		t.Fatalf("status --watch --json: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("--watch and --json are incompatible")) {
		t.Fatal("expected watch and json incompatible error")
	}
}

func TestLearnRecommend(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"AGENTS.md": "# Project\n"})
	// Create .rover/config.json in the repo
	roverDir := filepath.Join(repo, ".rover")
	if e := os.MkdirAll(roverDir, 0700); e != nil {
		t.Fatal(e)
	}
	cfg := map[string]any{
		"schema": model.Schema,
		"checks": []map[string]any{
			{"id": "test", "argv": []string{"/usr/bin/true"}, "timeout": "5s", "required": false, "parser": "exit-code"},
		},
		"policy": map[string]any{"require_review": false},
	}
	cb, _ := json.Marshal(cfg)
	if e := os.WriteFile(filepath.Join(roverDir, "config.json"), cb, 0600); e != nil {
		t.Fatal(e)
	}
	testutil.Git(t, repo, "add", "-A")
	testutil.Git(t, repo, "commit", "-m", "add config")
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "learn", "recommend", "--repo", repo, "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("learn recommend output is not valid JSON", out.String())
	}
}

func TestLearnShowInvalidID(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "learn", "show", "--id", "nonexistent", "--json"})
	if code != 2 {
		t.Fatalf("learn show nonexistent: expected exit 2, got %d", code)
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("learn show error should be valid JSON", out.String())
	}
}

func TestLearnInvalidSubcommand(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "learn", "bogus", "--json"})
	if code != 2 {
		t.Fatalf("learn bogus: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("learn recommend|dataset|evaluate|promote|show|revoke")) {
		t.Fatal("expected usage error for invalid learning subcommand")
	}
}

func TestIntegratePreview(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"AGENTS.md": "# Project\n"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "integrate", "--repo", repo, "--agent", "generic", "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("integrate output is not valid JSON", out.String())
	}
}

func TestArchiveBackupCheckMissingDir(t *testing.T) {
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "state"), "backup-check", "--from", filepath.Join(t.TempDir(), "nonexistent"), "--json"})
	if code != 2 {
		t.Fatalf("backup-check missing dir: expected exit 2, got %d", code)
	}
}

func TestArchiveBackupMissingTo(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "backup", "--repo", repo, "--json"})
	if code != 2 {
		t.Fatalf("backup missing --to: expected exit 2, got %d", code)
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("backup error should be valid JSON", out.String())
	}
}

func TestArchiveBackupToExisting(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	to := t.TempDir()
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "backup", "--repo", repo, "--to", to, "--json"})
	if code != 2 {
		t.Fatalf("backup to existing dir: expected exit 2, got %d", code)
	}
}

func TestMCPServerRequiresStore(t *testing.T) {
	repo, _ := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "nonexistent-store"), "mcp", "--repo", repo, "--json"})
	// Should fail because the store directory doesn't exist
	if code != 2 {
		t.Fatalf("mcp with bad store: expected exit 2, got %d", code)
	}
}

func TestServeRequiresStore(t *testing.T) {
	repo, _ := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", filepath.Join(t.TempDir(), "nonexistent-store"), "serve", "--repo", repo, "--json"})
	if code != 2 {
		t.Fatalf("serve with bad store: expected exit 2, got %d", code)
	}
}

func TestWorkflowRunMissingFile(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "workflow", "run", "--json"})
	if code != 2 {
		t.Fatalf("workflow run missing file: expected exit 2, got %d", code)
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("workflow run error should be valid JSON", out.String())
	}
}

func TestWorkflowCancelMissingID(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "workflow", "cancel", "--json"})
	if code != 2 {
		t.Fatalf("workflow cancel missing id: expected exit 2, got %d", code)
	}
}

func TestWorkflowStatus(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "workflow", "status", "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("workflow status output is not valid JSON", out.String())
	}
}

func TestRemoteNodeDeleteWithoutID(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "remote", "node-delete", "--json"})
	if code != 2 {
		t.Fatalf("remote node-delete without node: expected exit 2, got %d", code)
	}
}

func TestAttestSignMissingAllFlags(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "attest", "sign", "--json"})
	if code != 2 {
		t.Fatalf("attest sign missing flags: expected exit 2, got %d", code)
	}
}

func TestLearnDatasetMissingFile(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "learn", "dataset", "--file", filepath.Join(t.TempDir(), "missing.json"), "--json"})
	if code != 2 {
		t.Fatalf("learn dataset missing file: expected exit 2, got %d", code)
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("learn dataset error should be valid JSON", out.String())
	}
}

func TestLearnEvaluateMissingArgs(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "learn", "evaluate", "--json"})
	if code != 2 {
		t.Fatalf("learn evaluate missing args: expected exit 2, got %d", code)
	}
}

func TestRemoteTimeoutTooLarge(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "remote", "node-list", "--timeout", "25h", "--json"})
	if code != 2 {
		t.Fatalf("remote timeout too large: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("timeout must be between zero and 24h")) {
		t.Fatal("expected timeout range error")
	}
}

func TestReplayInvalidID(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "replay", "--id", "nonexistentinvestigation", "--json"})
	if code != 2 {
		t.Fatalf("replay invalid id: expected exit 2, got %d", code)
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("replay error should be valid JSON", out.String())
	}
}

func TestGrantCreateWithPositionalError(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "grant", "create", "--repo", repo, "--tool", "rover_status", "--note", "test", "bogus-positional", "--json"})
	if code != 2 {
		t.Fatalf("grant create positional: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("unexpected positional")) {
		t.Fatal("expected unexpected positional arguments error")
	}
}

func TestStatusReconcile(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "status", "--reconcile", "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), err.String())
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("status --reconcile output is not valid JSON", out.String())
	}
}

func TestCancelNonexistentID(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "cancel", "--id", "nonexistenttaskid", "--json"})
	if code != 2 {
		t.Fatalf("cancel nonexistent: expected exit 2, got %d", code)
	}
	if !json.Valid(out.Bytes()) {
		t.Fatal("cancel error should be valid JSON", out.String())
	}
}

func TestAttestSignWithValidKey(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	dir := t.TempDir()
	dir, _ = filepath.EvalSymlinks(dir)
	keyPath := filepath.Join(dir, "priv.key")
	pubPath := filepath.Join(dir, "pub.key")
	var out, errbytes bytes.Buffer
	app := New(&out, &errbytes)
	// Generate keypair
	code := app.Main(context.Background(), []string{"--state", s.Root, "attest", "keygen", "--key", keyPath, "--public-key", pubPath, "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), errbytes.String())
	}
	// Now sign an investigation
	out.Reset()
	code = app.Main(context.Background(), []string{"--state", s.Root, "attest", "sign", "--key", keyPath, "--id", "nonexistentinvestigation", "--json"})
	if code != 2 {
		t.Fatalf("attest sign nonexistent investigation: expected exit 2, got %d", code)
	}
}

func TestAttestVerifyWithValidKey(t *testing.T) {
	dir := t.TempDir()
	dir, _ = filepath.EvalSymlinks(dir)
	keyPath := filepath.Join(dir, "priv.key")
	pubPath := filepath.Join(dir, "pub.key")
	var out, errbytes bytes.Buffer
	app := New(&out, &errbytes)
	// Generate keypair
	state := filepath.Join(t.TempDir(), "state")
	code := app.Main(context.Background(), []string{"--state", state, "attest", "keygen", "--key", keyPath, "--public-key", pubPath, "--json"})
	if code != 0 {
		t.Fatal(code, out.String(), errbytes.String())
	}
	// Now verify with missing envelope file
	out.Reset()
	code = app.Main(context.Background(), []string{"--state", state, "attest", "verify", "--public-key", pubPath, "--file", filepath.Join(t.TempDir(), "missing.env"), "--json"})
	if code != 2 {
		t.Fatalf("attest verify missing envelope: expected exit 2, got %d", code)
	}
}

func TestRemoteUnknownOperationExplicit(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"value": "base"})
	var out, err bytes.Buffer
	app := New(&out, &err)
	code := app.Main(context.Background(), []string{"--state", s.Root, "remote", "bogus", "--json"})
	if code != 2 {
		t.Fatalf("remote bogus: expected exit 2, got %d", code)
	}
	if !bytes.Contains(out.Bytes(), []byte("unknown remote operation")) {
		t.Fatal("expected unknown remote operation error")
	}
}
