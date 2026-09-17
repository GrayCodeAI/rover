// Package execution runs explicit argv commands with bounded output. Local
// execution is advisory and is NOT a sandbox. No shell is inserted implicitly.
package execution

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"time"

	"github.com/GrayCodeAI/rover/internal/config"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/store"
)

const OutputLimit = 1 << 20

var imageRE = regexp.MustCompile(`^[a-zA-Z0-9][a-zA-Z0-9._/:\-]*@sha256:[0-9a-f]{64}$`)

type Capture struct {
	mu        sync.Mutex
	f         *os.File
	b         []byte
	seen      int64
	truncated bool
	writeErr  error
}

func newCapture(p string) (*Capture, error) {
	f, e := os.OpenFile(p, os.O_CREATE|os.O_WRONLY|os.O_EXCL, 0600)
	if e != nil {
		return nil, e
	}
	return &Capture{f: f, b: make([]byte, 0, 4096)}, nil
}
func (c *Capture) Write(p []byte) (int, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	n := len(p)
	c.seen += int64(n)
	keep := OutputLimit - len(c.b)
	if keep > n {
		keep = n
	}
	if keep < n {
		c.truncated = true
	}
	if keep > 0 {
		c.b = append(c.b, p[:keep]...)
		if _, e := c.f.Write(p[:keep]); e != nil {
			c.writeErr = e
			return 0, e
		}
	}
	return n, nil
}
func (c *Capture) close() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.f == nil {
		return c.writeErr
	}
	se := c.f.Sync()
	ce := c.f.Close()
	c.f = nil
	if c.writeErr != nil {
		return c.writeErr
	}
	if se != nil {
		return se
	}
	return ce
}

type Options struct {
	Dir         string
	OutputDir   string
	Argv        []string
	Timeout     time.Duration
	PassEnv     []string
	Mode        string
	Image       string
	Interactive bool
	Socket      string
}
type Result struct {
	Process                      model.ProcessResult
	Stdout, Stderr               []byte
	Executable, ExecutableSHA256 string
}

func cleanEnv(root string, pass []string) ([]string, error) {
	if e := config.Env(pass); e != nil {
		return nil, e
	}
	for _, d := range []string{root, filepath.Join(root, "home"), filepath.Join(root, "tmp"), filepath.Join(root, "cache"), filepath.Join(root, "gopath")} {
		if e := store.PrivateDir(d); e != nil {
			return nil, e
		}
	}
	vals := map[string]string{"PATH": os.Getenv("PATH"), "HOME": filepath.Join(root, "home"), "TMPDIR": filepath.Join(root, "tmp"), "GOCACHE": filepath.Join(root, "cache"), "GOPATH": filepath.Join(root, "gopath"), "GOTOOLCHAIN": "local", "GOPROXY": "off", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.DevNull, "GIT_TERMINAL_PROMPT": "0"}
	// Explicit pass_env is an authorization to inherit named values. Values are
	// not stored in metadata. This cannot stop local code reading the host.
	for _, k := range pass {
		if v, ok := os.LookupEnv(k); ok {
			vals[k] = v
		}
	}
	out := make([]string, 0, len(vals))
	for k, v := range vals {
		out = append(out, k+"="+v)
	}
	return out, nil
}
func DockerArgs(o Options, name string) ([]string, error) {
	if !imageRE.MatchString(o.Image) {
		return nil, errors.New("restricted-docker requires an explicit image@sha256:<64 hex> digest")
	}
	if strings.ContainsAny(o.Dir, ",\r\n") {
		return nil, errors.New("Docker mount source contains unsupported characters")
	}
	if o.Dir == "" || !filepath.IsAbs(o.Dir) || strings.Contains(o.Dir, "..") {
		return nil, errors.New("Docker mount source must be an absolute path without ..")
	}
	if len(o.PassEnv) > 0 {
		return nil, errors.New("host environment inheritance is disabled for restricted-docker checks")
	}
	a := []string{"run", "--name", name, "--pull=never", "--network=none", "--read-only", "--cap-drop=ALL", "--security-opt=no-new-privileges", "--pids-limit=128", "--memory=1g", "--cpus=1", "--user", fmt.Sprintf("%d:%d", os.Getuid(), os.Getgid()), "--mount", "type=bind,src=" + o.Dir + ",dst=/work", "--tmpfs", "/tmp:rw,nosuid,nodev,size=256m,mode=1777", "--workdir", "/work", "--env", "HOME=/tmp", "--env", "GOCACHE=/tmp/go-cache", "--env", "GOPATH=/tmp/go-path", "--env", "GOPROXY=off", "--env", "GOTOOLCHAIN=local", o.Image}
	return append(a, o.Argv...), nil
}
func Admit(mode string, allowLocal bool, image string) error {
	switch mode {
	case "local-advisory":
		if !allowLocal {
			return errors.New("local code execution requires --allow-local; local mode is not a sandbox")
		}
	case "restricted-docker":
		if !imageRE.MatchString(image) {
			return errors.New("pinned Docker image digest required")
		}
		if _, e := exec.LookPath("docker"); e != nil {
			return errors.New("Docker is unavailable; refusing fallback to local execution")
		}
	default:
		return fmt.Errorf("unsupported trust mode %q; protected acceptance is not implemented", mode)
	}
	return nil
}
func Run(parent context.Context, o Options) (r Result, err error) {
	if e := config.Argv(o.Argv); e != nil {
		return r, e
	}
	if o.Timeout <= 0 {
		return r, errors.New("positive timeout required")
	}
	if o.Interactive {
		return runPTY(parent, o)
	}
	if e := store.PrivateDir(o.OutputDir); e != nil {
		return r, e
	}
	out, e := newCapture(filepath.Join(o.OutputDir, "stdout.log"))
	if e != nil {
		return r, e
	}
	er, e := newCapture(filepath.Join(o.OutputDir, "stderr.log"))
	if e != nil {
		out.close()
		return r, e
	}
	defer func() {
		oe := out.close()
		ee := er.close()
		if err == nil {
			if oe != nil {
				err = oe
			} else if ee != nil {
				err = ee
			}
		}
	}()
	argv := o.Argv
	name := ""
	switch o.Mode {
	case "local-advisory":
	case "restricted-docker":
		name = model.ID("rover")
		a, e := DockerArgs(o, name)
		if e != nil {
			return r, e
		}
		argv = append([]string{"docker"}, a...)
	default:
		return r, errors.New("unknown executor mode")
	}
	ctx, cancel := context.WithTimeout(parent, o.Timeout)
	defer cancel()
	cmd := exec.CommandContext(ctx, argv[0], argv[1:]...)
	cmd.Dir = o.Dir
	cmd.Stdin = nil
	cmd.Stdout = out
	cmd.Stderr = er
	cmd.WaitDelay = time.Second
	env, e := cleanEnv(filepath.Join(o.OutputDir, "environment"), o.PassEnv)
	if e != nil {
		return r, e
	}
	cmd.Env = env
	configureProcess(cmd)
	p := cmd.Path
	if !filepath.IsAbs(p) {
		if strings.ContainsRune(p, filepath.Separator) {
			p = filepath.Join(o.Dir, p)
		} else if v, e := exec.LookPath(p); e == nil {
			p = v
		}
	}
	r.Executable = p
	if f, e := os.Open(p); e == nil {
		h := sha256.New()
		if _, e := io.Copy(h, io.LimitReader(f, 128<<20)); e == nil {
			r.ExecutableSHA256 = hex.EncodeToString(h.Sum(nil))
		}
		f.Close()
	}
	r.Process.StartedAt = model.Now()
	r.Process.ExitCode = -1
	e = cmd.Run()
	// Only sweep the process group on timeout/cancellation where descendants
	// may linger. An unconditional kill after a clean wait risks signalling a
	// recycled PGID. Processes deliberately escaping the group still require
	// real OS/container isolation.
	if ctx.Err() != nil || parent.Err() != nil {
		cleanupProcess(cmd)
	}
	r.Process.FinishedAt = model.Now()
	if cmd.ProcessState != nil {
		r.Process.ExitCode = cmd.ProcessState.ExitCode()
	}
	if e != nil {
		r.Process.Error = e.Error()
	}
	r.Process.TimedOut = errors.Is(ctx.Err(), context.DeadlineExceeded)
	r.Process.Cancelled = parent.Err() != nil
	if name != "" {
		cleanCtx, done := context.WithTimeout(context.Background(), 10*time.Second)
		defer done()
		cleanup := exec.CommandContext(cleanCtx, "docker", "rm", "--force", name)
		cleanup.Env = env
		if ce := cleanup.Run(); ce != nil {
			r.Process.Error += fmt.Sprintf("; container cleanup not confirmed: %v", ce)
		}
	}
	r.Stdout = append([]byte{}, out.b...)
	r.Stderr = append([]byte{}, er.b...)
	r.Process.StdoutSHA256 = model.Digest(r.Stdout)
	r.Process.StderrSHA256 = model.Digest(r.Stderr)
	r.Process.StdoutBytes = out.seen
	r.Process.StderrBytes = er.seen
	r.Process.Truncated = out.truncated || er.truncated
	return r, nil
}

// SafeText keeps human-facing noninteractive output from executing terminal
// controls. JSON encoding also escapes control characters.
func SafeText(s string) string {
	return strings.Map(func(r rune) rune {
		if r == '\n' || r == '\t' {
			return r
		}
		if r < 32 || (r >= 0x7f && r <= 0x9f) {
			return -1
		}
		return r
	}, s)
}
