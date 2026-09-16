package execution

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func opts(t *testing.T, script string) Options {
	t.Helper()
	root := t.TempDir()
	dir := filepath.Join(root, "work")
	os.Mkdir(dir, 0700)
	return Options{Dir: dir, OutputDir: filepath.Join(root, "out"), Argv: []string{"/bin/sh", "-c", script}, Timeout: 2 * time.Second, Mode: "local-advisory"}
}
func TestProcessResults(t *testing.T) {
	for _, tc := range []struct {
		name, script string
		exit         int
	}{{"pass", "printf hello", 0}, {"nonzero", "printf err >&2; exit 7", 7}} {
		t.Run(tc.name, func(t *testing.T) {
			r, e := Run(context.Background(), opts(t, tc.script))
			if e != nil || r.Process.ExitCode != tc.exit {
				t.Fatal(e, r.Process)
			}
			if r.Process.StdoutSHA256 == "" || r.Process.StderrSHA256 == "" {
				t.Fatal("missing provenance")
			}
		})
	}
}
func TestTimeout(t *testing.T) {
	o := opts(t, "sleep 10")
	o.Timeout = 60 * time.Millisecond
	start := time.Now()
	r, e := Run(context.Background(), o)
	if e != nil || !r.Process.TimedOut || time.Since(start) > 3*time.Second {
		t.Fatal(e, r.Process)
	}
}
func TestCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	time.AfterFunc(40*time.Millisecond, cancel)
	r, e := Run(ctx, opts(t, "sleep 10"))
	if e != nil || !r.Process.Cancelled {
		t.Fatal(e, r.Process)
	}
}
func TestOutputBound(t *testing.T) {
	r, e := Run(context.Background(), opts(t, "head -c 1200000 /dev/zero"))
	if e != nil {
		t.Fatal(e)
	}
	if !r.Process.Truncated || len(r.Stdout) != OutputLimit || r.Process.StdoutBytes != 1200000 {
		t.Fatal(r.Process, len(r.Stdout))
	}
}
func TestEnvironmentIsExplicit(t *testing.T) {
	t.Setenv("ROVER_TEST_SECRET", "value")
	o := opts(t, `test -z "$ROVER_TEST_SECRET"`)
	r, e := Run(context.Background(), o)
	if e != nil || r.Process.ExitCode != 0 {
		t.Fatal(e, r.Process)
	}
	o = opts(t, `test "$ROVER_TEST_SECRET" = value`)
	o.PassEnv = []string{"ROVER_TEST_SECRET"}
	r, e = Run(context.Background(), o)
	if e != nil || r.Process.ExitCode != 0 {
		t.Fatal(e, r.Process)
	}
}
func TestDockerConstraints(t *testing.T) {
	o := opts(t, "true")
	o.Image = "example/image@sha256:" + strings.Repeat("a", 64)
	o.Mode = "restricted-docker"
	a, e := DockerArgs(o, "rover_test")
	if e != nil {
		t.Fatal(e)
	}
	s := strings.Join(a, " ")
	for _, want := range []string{"--network=none", "--pull=never", "--read-only", "--cap-drop=ALL", "--security-opt=no-new-privileges", "--user", "--pids-limit=128"} {
		if !strings.Contains(s, want) {
			t.Error(want)
		}
	}
	o.Image = "example:latest"
	if _, e = DockerArgs(o, "x"); e == nil {
		t.Fatal("unpinned image accepted")
	}
}
func TestTrustAdmission(t *testing.T) {
	if Admit("local-advisory", false, "") == nil {
		t.Fatal("missing grant accepted")
	}
	if Admit("protected", true, "") == nil {
		t.Fatal("unsupported mode accepted")
	}
	if e := Admit("local-advisory", true, ""); e != nil {
		t.Fatal(e)
	}
}
func TestSafeOutput(t *testing.T) {
	x := SafeText("a\x1b[31mb\r\x00c\x9bd\n")
	if strings.ContainsAny(x, "\x1b\r\x00\u009b") {
		t.Fatal(x)
	}
}
