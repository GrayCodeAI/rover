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
