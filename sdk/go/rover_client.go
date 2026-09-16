// Package rover is a small standard-library client for Rover's CLI (v0.x API).
// No shell, automatic retries, provider credentials, or acceptance inference.
// A CLI submission may continue after a client timeout; reconcile before retrying.
package rover

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os/exec"
	"path/filepath"
	"time"
)

// RoverError is a protocol, invocation or bounded-output failure.
type RoverError struct{ msg string }

func (e *RoverError) Error() string { return e.msg }

// Result is a bounded JSON result from the CLI.
type Result struct {
	ExitCode int
	Data     any
}

// Decision returns data["decision"] when present.
func (r Result) Decision() string {
	if m, ok := r.Data.(map[string]any); ok {
		if s, ok := m["decision"].(string); ok {
			return s
		}
	}
	return ""
}

// Rover invokes an explicit Rover binary with argv, never a shell.
type Rover struct {
	Binary    string
	State     string
	Timeout   time.Duration
	MaxOutput int
}

// New creates a Rover client.
func New(binary, state string, opts ...Option) (*Rover, error) {
	absBin, err := filepath.Abs(binary)
	if err != nil {
		return nil, err
	}
	absState, err := filepath.Abs(state)
	if err != nil {
		return nil, err
	}
	r := &Rover{Binary: absBin, State: absState, Timeout: 60 * time.Second, MaxOutput: 16 << 20}
	for _, o := range opts {
		o(r)
	}
	if r.Timeout <= 0 || r.MaxOutput < 1024 {
		return nil, errors.New("positive timeout and output bound required")
	}
	return r, nil
}

type Option func(*Rover)

func WithTimeout(d time.Duration) Option { return func(r *Rover) { r.Timeout = d } }
func WithMaxOutput(n int) Option         { return func(r *Rover) { r.MaxOutput = n } }

// Call invokes: rover --state <state> <args> --json
func (r *Rover) Call(args []string) (Result, error) {
	for _, a := range args {
		if a == "" {
			return Result{}, &RoverError{"args must be non-empty strings"}
		}
	}
	argv := append([]string{"--state", r.State}, args...)
	argv = append(argv, "--json")
	ctx, cancel := context.WithTimeout(context.Background(), r.Timeout)
	defer cancel()
	cmd := exec.CommandContext(ctx, r.Binary, argv...)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	err := cmd.Run()
	if ctx.Err() == context.DeadlineExceeded {
		return Result{}, &RoverError{"Client timed out. An admitted task may still be running; inspect state before retrying."}
	}
	if stdout.Len() > r.MaxOutput {
		return Result{}, &RoverError{"Rover output exceeded configured client limit"}
	}
	if err != nil {
		// still try to parse JSON; Rover may return JSON on non-zero exit
		if stdout.Len() == 0 {
			return Result{}, &RoverError{fmt.Sprintf("Rover did not return JSON (exit %d): %s", exitCode(cmd), truncate(stderr.String(), 400))}
		}
	}
	var data any
	if err := json.Unmarshal(stdout.Bytes(), &data); err != nil {
		return Result{}, &RoverError{fmt.Sprintf("Rover did not return JSON (exit %d): %s", exitCode(cmd), truncate(stderr.String(), 400))}
	}
	return Result{ExitCode: exitCode(cmd), Data: data}, nil
}

func exitCode(cmd *exec.Cmd) int {
	if cmd.ProcessState == nil {
		return -1
	}
	return cmd.ProcessState.ExitCode()
}

func truncate(s string, n int) string {
	if len(s) > n {
		return s[:n]
	}
	return s
}

func (r *Rover) Inspect(repo string, opts ...InspectOption) (Result, error) {
	c := inspectConfig{base: "HEAD"}
	for _, o := range opts {
		o(&c)
	}
	args := []string{"inspect", "--repo", repo, "--base", c.base}
	if c.worktree {
		args = append(args, "--worktree")
	}
	return r.Call(args)
}

type inspectConfig struct {
	base     string
	worktree bool
}
type InspectOption func(*inspectConfig)

func WithBase(base string) InspectOption { return func(c *inspectConfig) { c.base = base } }
func WithWorktree(v bool) InspectOption  { return func(c *inspectConfig) { c.worktree = v } }
func (r *Rover) Verify(repo string, opts ...VerifyOption) (Result, error) {
	c := verifyConfig{base: "HEAD"}
	for _, o := range opts {
		o(&c)
	}
	args := []string{"verify", "--repo", repo, "--base", c.base}
	if c.allowLocal {
		args = append(args, "--allow-local")
	}
	if c.worktree {
		args = append(args, "--worktree")
	}
	return r.Call(args)
}

type verifyConfig struct {
	base       string
	allowLocal bool
	worktree   bool
}
type VerifyOption func(*verifyConfig)

func WithVerifyBase(base string) VerifyOption { return func(c *verifyConfig) { c.base = base } }
func WithAllowLocal(v bool) VerifyOption      { return func(c *verifyConfig) { c.allowLocal = v } }
func WithVerifyWorktree(v bool) VerifyOption  { return func(c *verifyConfig) { c.worktree = v } }

func (r *Rover) RunTask(taskFile string, opts ...RunTaskOption) (Result, error) {
	c := runTaskConfig{}
	for _, o := range opts {
		o(&c)
	}
	args := []string{"task", "run", "--file", taskFile}
	if c.allowLocal {
		args = append(args, "--allow-local")
	}
	if c.key != "" {
		args = append(args, "--key", c.key)
	}
	return r.Call(args)
}

type runTaskConfig struct {
	allowLocal bool
	key        string
}
type RunTaskOption func(*runTaskConfig)

func WithRunAllowLocal(v bool) RunTaskOption { return func(c *runTaskConfig) { c.allowLocal = v } }
func WithKey(key string) RunTaskOption       { return func(c *runTaskConfig) { c.key = key } }

func (r *Rover) Status(taskID ...string) (Result, error) {
	if len(taskID) > 0 && taskID[0] != "" {
		return r.Call([]string{"status", "--id", taskID[0]})
	}
	return r.Call([]string{"status"})
}

func (r *Rover) Report(investigationID string) (Result, error) {
	return r.Call([]string{"report", "--id", investigationID})
}
