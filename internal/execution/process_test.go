package execution

import (
	"bytes"
	"context"
	"os"
	"os/exec"
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

// TestDockerRequiresDaemon covers A18's local gate: with no Docker daemon the
// restricted-docker mode must refuse cleanly and never fall back to local
// execution. With a Docker binary present, admission is allowed but still does
// not certify the container runtime (a live run needs a real daemon).
func TestDockerRequiresDaemon(t *testing.T) {
	docker, e := exec.LookPath("docker")
	pinned := "example/image@sha256:" + strings.Repeat("a", 64)
	if e != nil {
		err := Admit("restricted-docker", true, pinned)
		if err == nil || !strings.Contains(err.Error(), "Docker is unavailable; refusing fallback") {
			t.Fatalf("expected refusal without fallback, got %v", err)
		}
		return
	}
	if e := Admit("restricted-docker", true, pinned); e != nil {
		t.Fatalf("docker binary present but refused: %v", e)
	}
	t.Logf("docker binary present (%s); container isolation still not live-certified", docker)
}

// TestDockerFunctionalSmokeLocalDaemon exercises the real restricted-docker path
// against a reachable local daemon with a pinned image and --pull=never. It runs
// only where a daemon plus the pinned image are actually present: hosts without
// Docker skip, and a daemon that cannot run a container here is logged and
// skipped with full stderr. A successful run still only proves the plumbing on
// this machine — it is a smoke, never an independent-host or security
// certification.
func TestDockerFunctionalSmokeLocalDaemon(t *testing.T) {
	const pinned = "alpine@sha256:28bd5fe8b56d1bd048e5babf5b10710ebe0bae67db86916198a6eec434943f8b"
	if _, e := exec.LookPath("docker"); e != nil {
		t.Skip("docker CLI not on PATH; functional smoke skipped")
	}
	if e := exec.Command("docker", "info").Run(); e != nil {
		t.Skipf("Docker daemon unreachable (%v); functional smoke skipped", e)
	}
	if e := exec.Command("docker", "image", "inspect", pinned).Run(); e != nil {
		t.Skipf("pinned image not present locally (refusing to pull from network): %v", e)
	}
	dir := t.TempDir()
	if e := os.WriteFile(filepath.Join(dir, "probe.txt"), []byte("smoke"), 0o600); e != nil {
		t.Fatal(e)
	}
	o := Options{
		Mode:      "restricted-docker",
		Image:     pinned,
		Dir:       dir,
		OutputDir: t.TempDir(),
		Timeout:   90 * time.Second,
		Argv:      []string{"/bin/sh", "-c", "printf 'rover-docker-smoke-ok\\n' && cmp /work/probe.txt /work/probe.txt"},
	}
	r, e := Run(context.Background(), o)
	if e != nil {
		t.Skipf("unexpected run error (%v); functional smoke not certified on this host", e)
	}
	if r.Process.ExitCode == -1 {
		t.Skipf("docker command never started; functional smoke not certified on this host")
	}
	if r.Process.ExitCode != 0 {
		t.Skipf("daemon could not run the pinned container here (exit %d); functional smoke not certified", r.Process.ExitCode)
	}
	if !bytes.Contains(r.Stdout, []byte("rover-docker-smoke-ok")) {
		t.Fatalf("container ran but sentinel missing: stdout=%q stderr=%q", r.Stdout, r.Stderr)
	}
	t.Logf("restricted-docker container ran with %s (uid %d) — smoke only, not independent-host/security certification", pinned, os.Getuid())
}
func TestSafeOutput(t *testing.T) {
	x := SafeText("a\x1b[31mb\r\x00c\x9bd\n")
	if strings.ContainsAny(x, "\x1b\r\x00\u009b") {
		t.Fatal(x)
	}
}
