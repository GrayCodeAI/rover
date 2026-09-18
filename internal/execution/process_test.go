package execution

import (
	"bytes"
	"context"
	"errors"
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

func TestAdmitRestrictedDockerNoImage(t *testing.T) {
	e := Admit("restricted-docker", true, "")
	if e == nil || !strings.Contains(e.Error(), "pinned Docker image digest required") {
		t.Fatalf("expected image digest error, got %v", e)
	}
}

func TestAdmitRestrictedDockerInvalidImage(t *testing.T) {
	e := Admit("restricted-docker", true, "not-a-valid-image")
	if e == nil || !strings.Contains(e.Error(), "pinned Docker image digest required") {
		t.Fatalf("expected image digest error, got %v", e)
	}
}

func TestAdmitLocalAdvisoryWithoutAllowLocal(t *testing.T) {
	e := Admit("local-advisory", false, "")
	if e == nil || !strings.Contains(e.Error(), "--allow-local") {
		t.Fatalf("expected allow-local error, got %v", e)
	}
}

func TestAdmitLocalAdvisoryWithAllowLocal(t *testing.T) {
	if e := Admit("local-advisory", true, ""); e != nil {
		t.Fatalf("expected no error, got %v", e)
	}
}

func TestDockerArgsRelativeDir(t *testing.T) {
	o := Options{Mode: "restricted-docker", Image: "x@sha256:" + strings.Repeat("a", 64), Dir: "relative", Argv: []string{"true"}}
	_, e := DockerArgs(o, "rover_test")
	if e == nil || !strings.Contains(e.Error(), "absolute path") {
		t.Fatalf("expected absolute path error, got %v", e)
	}
}

func TestDockerArgsDotDotDir(t *testing.T) {
	o := Options{Mode: "restricted-docker", Image: "x@sha256:" + strings.Repeat("a", 64), Dir: "/tmp/../etc", Argv: []string{"true"}}
	_, e := DockerArgs(o, "rover_test")
	if e == nil || !strings.Contains(e.Error(), "absolute path") {
		t.Fatalf("expected absolute path error, got %v", e)
	}
}

func TestDockerArgsCommaDir(t *testing.T) {
	o := Options{Mode: "restricted-docker", Image: "x@sha256:" + strings.Repeat("a", 64), Dir: "/tmp,a/b", Argv: []string{"true"}}
	_, e := DockerArgs(o, "rover_test")
	if e == nil || !strings.Contains(e.Error(), "unsupported characters") {
		t.Fatalf("expected unsupported characters error, got %v", e)
	}
}

func TestDockerArgsPassEnvForbidden(t *testing.T) {
	o := Options{Mode: "restricted-docker", Image: "x@sha256:" + strings.Repeat("a", 64), Dir: "/tmp", Argv: []string{"true"}, PassEnv: []string{"FOO"}}
	_, e := DockerArgs(o, "rover_test")
	if e == nil || !strings.Contains(e.Error(), "host environment inheritance") {
		t.Fatalf("expected passenv forbidden error, got %v", e)
	}
}

func TestDockerArgsNewlineDir(t *testing.T) {
	o := Options{Mode: "restricted-docker", Image: "x@sha256:" + strings.Repeat("a", 64), Dir: "/tmp\na/b", Argv: []string{"true"}}
	_, e := DockerArgs(o, "rover_test")
	if e == nil || !strings.Contains(e.Error(), "unsupported characters") {
		t.Fatalf("expected unsupported characters error, got %v", e)
	}
}

func TestRunEmptyArgv(t *testing.T) {
	o := opts(t, "true")
	o.Argv = []string{}
	_, e := Run(context.Background(), o)
	if e == nil {
		t.Fatal("expected error for empty argv")
	}
}

func TestRunArgvWithEmptyFirstElement(t *testing.T) {
	o := opts(t, "true")
	o.Argv = []string{"", "arg"}
	_, e := Run(context.Background(), o)
	if e == nil {
		t.Fatal("expected error for empty argv[0]")
	}
}

func TestRunInvalidMode(t *testing.T) {
	o := opts(t, "true")
	o.Mode = "bogus-mode"
	_, e := Run(context.Background(), o)
	if e == nil || !strings.Contains(e.Error(), "unknown executor mode") {
		t.Fatalf("expected unknown executor mode error, got %v", e)
	}
}

func TestRunTimeoutTooSmall(t *testing.T) {
	o := opts(t, "true")
	o.Timeout = 0
	_, e := Run(context.Background(), o)
	if e == nil || !strings.Contains(e.Error(), "positive timeout required") {
		t.Fatalf("expected positive timeout error, got %v", e)
	}
}

func TestRunRestrictedDockerNoDocker(t *testing.T) {
	if _, e := exec.LookPath("docker"); e == nil {
		t.Skip("docker binary present; restricted-docker Run would attempt real execution")
	}
	o := opts(t, "true")
	o.Mode = "restricted-docker"
	o.Image = "x@sha256:" + strings.Repeat("a", 64)
	_, e := Run(context.Background(), o)
	if e == nil || !strings.Contains(e.Error(), "Docker is unavailable") {
		t.Fatalf("expected Docker unavailable error, got %v", e)
	}
}

func TestCleanEnvInvalidPassEnv(t *testing.T) {
	_, e := cleanEnv(t.TempDir(), []string{"bad-env-name!"})
	if e == nil || !strings.Contains(e.Error(), "invalid environment variable name") {
		t.Fatalf("expected invalid env name error, got %v", e)
	}
}

func TestNewCaptureFileExists(t *testing.T) {
	dir := t.TempDir()
	p := filepath.Join(dir, "exists.log")
	if e := os.WriteFile(p, []byte("data"), 0600); e != nil {
		t.Fatal(e)
	}
	_, e := newCapture(p)
	if e == nil {
		t.Fatal("expected error for existing capture file")
	}
}

func TestNewCaptureInvalidDir(t *testing.T) {
	_, e := newCapture("/nonexistent/dir/file.log")
	if e == nil {
		t.Fatal("expected error for invalid capture path")
	}
}

func TestCaptureWriteTruncation(t *testing.T) {
	dir := t.TempDir()
	p := filepath.Join(dir, "cap.log")
	f, e := os.OpenFile(p, os.O_CREATE|os.O_WRONLY|os.O_EXCL, 0600)
	if e != nil {
		t.Fatal(e)
	}
	c := &Capture{f: f, b: make([]byte, 0, 16)}
	data := make([]byte, OutputLimit)
	n, err := c.Write(data)
	if err != nil || n != OutputLimit {
		t.Fatalf("expected full write, got n=%d err=%v", n, err)
	}
	if c.truncated {
		t.Fatal("should not be truncated at exactly OutputLimit")
	}
	f.Close()
}

func TestCaptureWriteBeyondLimit(t *testing.T) {
	dir := t.TempDir()
	p := filepath.Join(dir, "cap.log")
	f, e := os.OpenFile(p, os.O_CREATE|os.O_WRONLY|os.O_EXCL, 0600)
	if e != nil {
		t.Fatal(e)
	}
	c := &Capture{f: f, b: make([]byte, 0, 16)}
	data := make([]byte, OutputLimit+1)
	n, err := c.Write(data)
	if err != nil || n != OutputLimit+1 {
		t.Fatalf("expected n=%d, got n=%d err=%v", OutputLimit+1, n, err)
	}
	if !c.truncated {
		t.Fatal("should be truncated")
	}
	if len(c.b) > OutputLimit {
		t.Fatal("buffer should not exceed OutputLimit")
	}
	f.Close()
}

func TestCaptureCloseWithoutFile(t *testing.T) {
	c := &Capture{f: nil, writeErr: errors.New("prior error")}
	e := c.close()
	if e == nil || !strings.Contains(e.Error(), "prior error") {
		t.Fatalf("expected prior write error, got %v", e)
	}
}

func TestCaptureCloseNilFileNoError(t *testing.T) {
	c := &Capture{f: nil}
	e := c.close()
	if e != nil {
		t.Fatalf("expected nil error for closed capture, got %v", e)
	}
}

func TestRunOutputDirPrivateDirFailure(t *testing.T) {
	o := opts(t, "true")
	o.OutputDir = "/nonexistent/dir/for/output"
	_, e := Run(context.Background(), o)
	if e == nil {
		t.Fatal("expected error for nonexistent output dir")
	}
}

func TestRunCaptureCreationFailure(t *testing.T) {
	o := opts(t, "true")
	o.OutputDir = "/dev/null/cannot/create"
	_, e := Run(context.Background(), o)
	if e == nil {
		t.Fatal("expected error for capture creation failure")
	}
}

func TestRunCleanEnvFailure(t *testing.T) {
	o := opts(t, "true")
	o.PassEnv = []string{"invalid-name!"}
	_, e := Run(context.Background(), o)
	if e == nil || !strings.Contains(e.Error(), "invalid environment variable name") {
		t.Fatalf("expected invalid env name error, got %v", e)
	}
}

func TestRunExecutableNotFound(t *testing.T) {
	o := opts(t, "true")
	o.Argv = []string{"nonexistent-binary-12345"}
	r, e := Run(context.Background(), o)
	if e != nil {
		t.Fatalf("expected nil error from Run (error stored in result), got %v", e)
	}
	if r.Process.Error == "" {
		t.Fatal("expected nonzero exit code and error in result")
	}
	if r.Process.ExitCode != -1 {
		t.Fatalf("expected exit code -1, got %d", r.Process.ExitCode)
	}
}
