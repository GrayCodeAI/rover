// Package cli is a thin client for Rover's domain services. Human output is
// sanitized; --json reports the same typed records without ANSI formatting.
package cli

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"github.com/GrayCodeAI/rover/internal/agents"
	"github.com/GrayCodeAI/rover/internal/assurance"
	"github.com/GrayCodeAI/rover/internal/config"
	"github.com/GrayCodeAI/rover/internal/execution"
	"github.com/GrayCodeAI/rover/internal/install"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/publish"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/store"
	"github.com/GrayCodeAI/rover/internal/tasks"
)

const help = `Rover — terminal-first agent execution and evidence (alpha)

Usage: rover [--state DIRECTORY] COMMAND [flags]

State defaults to $ROVER_HOME or ~/.config/rover; --state overrides it.
Keep state outside all repositories.

Shortcuts (one command instead of three):
  check --repo . --worktree --allow-local
                                    Inspect + verify + decision + next step
  do --file TASK.json --allow-local
                                    Run a task in the foreground, then report
Aliases: st=status, lg=logs, wf=workflow, rep=report

Read/setup:
  version                        Version and build environment
  doctor                         Inspect available tools; executes no repository code
  init [--repo .] [--apply]       Preview or create .rover/config.json; never overwrite
  inspect [--base HEAD]           Compare exact Git source states
  agents                         Implemented capability manifest

Verification (requires an execution grant):
  verify --base REF [--config FILE] --allow-local
  verify --base REF --mode restricted-docker --image IMAGE@sha256:DIGEST
  report --id INVESTIGATION      Read persisted evidence
  review --id INVESTIGATION --note TEXT

Agent workflows:
  task run --file TASK.json --allow-local [--foreground] [--key KEY]
  status [--id TASK] [--reconcile]
  logs --id TASK [--follow] [--stderr]
  cancel --id TASK
  ui [--watch]                   Plain-text dashboard
  tui                            Keyboard dashboard (Linux terminal)
  attach --id TASK               Interactive PTY attachment; Ctrl-] detaches
  workflow run --file FILE --allow-local [--foreground] [--key KEY]
  workflow status|cancel --id ID

Other:
  events --id RECORD             Event index
  export --kind KIND             Export a bounded record page
  outcome --id INVESTIGATION --label LABEL --note TEXT
  prove regression --file SPEC  Explicit counterfactual testcase investigation
  mutate --file SPEC            Approved finite textual mutation campaign
  replay --id INVESTIGATION     Re-run retained candidate and configuration
  diff --id INVESTIGATION       Applicable patch from retained source snapshots
  context search|bundle         Bounded snapshot-scoped source context
  memory put|list|delete         Explicit expiring repository notes
  integrate --repo . --agent generic [--apply] [--undo RECEIPT]
  mcp --repo .                  Read-only stdio tool server by default
  serve --repo .                Scoped MCP HTTP endpoint; grants required
  grant create|list|revoke       Local-operator access-grant administration
  remote node-add|node-list|node-delete|tools|call
  backup --to NEW_DIRECTORY     Consistent metadata + retained objects
  backup-check --from DIRECTORY
  restore --from BACKUP --to NEW_STATE
  attest keygen|sign|verify      Explicit-key local evidence signatures
  learn recommend|dataset|evaluate|promote|show|revoke
  limits [--max-agents N]        Same-store admission limit

  doctor --verify                Verify SOURCE_MANIFEST.json against local files (not a signed installer)
  publish --id ID --to DIR       Local advisory publisher (writes to DIR, not protected CI)

All commands support --json where applicable.
Verification exits: 0 policy accepted; 1 blocked; 2 error/inconclusive; 3 review required.
Task dispatch exit 0 means dispatched, NOT accepted software.
Local advisory execution is NOT a sandbox. No automatic push, merge, or deploy.
`

type App struct{ Out, Err io.Writer }

func New(out, err io.Writer) *App { return &App{Out: out, Err: err} }
func (a *App) fs(name string) *flag.FlagSet {
	f := flag.NewFlagSet(name, flag.ContinueOnError)
	f.SetOutput(a.Err)
	return f
}
func (a *App) emit(v any) error {
	e := json.NewEncoder(a.Out)
	e.SetIndent("", "  ")
	return e.Encode(v)
}
func (a *App) verifyInstall(root, manifest string) error { return install.Verify(root, manifest) }
func (a *App) Main(ctx context.Context, args []string) int {
	jsonMode := false
	for _, x := range args {
		if x == "--json" {
			jsonMode = true
		}
	}
	code, e := a.run(ctx, args)
	if e != nil {
		if jsonMode {
			_ = a.emit(map[string]any{"schema": model.Schema, "error": e.Error()})
		} else {
			fmt.Fprintln(a.Err, "rover:", execution.SafeText(e.Error()))
		}
		if code == 0 {
			code = 2
		}
	}
	return code
}
func (a *App) run(ctx context.Context, args []string) (int, error) {
	root, defaultErr := store.DefaultRoot()
	var e error
	f := a.fs("rover")
	f.StringVar(&root, "state", root, "private state directory outside repositories")
	if e = f.Parse(args); e != nil {
		return 2, e
	}
	if root == "" && defaultErr != nil {
		return 2, defaultErr
	}
	args = f.Args()
	if len(args) == 0 || args[0] == "help" || args[0] == "--help" || args[0] == "-h" {
		fmt.Fprint(a.Out, help)
		return 0, nil
	}
	command := args[0]
	args = args[1:]
	// Short aliases for the most-used read commands. Full names keep working.
	if short, ok := map[string]string{"st": "status", "lg": "logs", "wf": "workflow", "rep": "report"}[command]; ok {
		command = short
	}
	if command == "version" {
		return 0, a.emit(map[string]string{"name": "Rover", "version": model.Version, "schema": model.Schema, "go": runtime.Version(), "sqlite": store.SQLiteVersion(), "os": runtime.GOOS, "arch": runtime.GOARCH})
	}
	if command == "doctor" {
		verify := contains(args, "--verify")
		if verify {
			// Verify SOURCE_MANIFEST.json in the rover source root (two levels up from this file when run from repo).
			// For installed binaries, manifest is not present — this is local-source verification, not a signed installer.
			roots := []string{".", filepath.Join(filepath.Dir(os.Args[0]), ".."), filepath.Join(filepath.Dir(os.Args[0]), "../..")}
			var lastErr error
			for _, r := range roots {
				abs, _ := filepath.Abs(r)
				mp := filepath.Join(abs, "SOURCE_MANIFEST.json")
				if _, e := os.Stat(mp); e == nil {
					if e = func() error {
						// Use internal/install.Verify
						// Import is deferred to avoid cycle; call via helper.
						return a.verifyInstall(abs, mp)
					}(); e == nil {
						return 0, a.emit(map[string]any{"schema": model.Schema, "verified": true, "manifest": mp, "note": "local manifest check; not a signed public installer"})
					} else {
						lastErr = e
					}
				}
			}
			if lastErr != nil {
				return 2, fmt.Errorf("install verification failed: %w", lastErr)
			}
			return 2, errors.New("SOURCE_MANIFEST.json not found for verification")
		}
		tools := map[string]any{}
		for _, n := range []string{"git", "go", "python3", "docker", "tmux", "codex", "claude"} {
			p, e := exec.LookPath(n)
			tools[n] = map[string]any{"available": e == nil, "path": p}
		}
		return 0, a.emit(map[string]any{"schema": model.Schema, "tools": tools, "state_directory": root, "trust_modes": []string{"local-advisory", "restricted-docker (adapter; live testing pending)"}, "warning": "tool presence is not integration certification; provider credentials, non-Linux platforms and live Docker need environment-specific validation; local mode is not a sandbox"})
	}
	if command == "agents" {
		return 0, a.emit(agents.List())
	}
	if command == "init" {
		return a.init(ctx, args)
	}
	switch command {
	case "agent", "attach", "tui", "workflow", "__workflow", "prove", "mutate", "replay", "diff", "context", "memory", "limits", "mcp", "serve", "grant", "backup", "backup-check", "restore", "attest", "integrate", "learn", "remote":
		ex := &extendedApp{App: a, root: root, jsonMode: contains(args, "--json")}
		return ex.advanced(ctx, append([]string{command}, args...)), nil
	}
	s, e := store.Open(root)
	if e != nil {
		return 2, e
	}
	defer s.Close()
	switch command {
	case "inspect", "verify":
		return a.inspectVerify(ctx, s, command, args)
	case "check":
		return a.check(ctx, s, args)
	case "do":
		return a.do(ctx, s, args)
	case "task":
		return a.task(ctx, s, args)
	case "__worker":
		f := a.fs("worker")
		id := f.String("run", "", "run identifier")
		if e = f.Parse(args); e != nil {
			return 2, e
		}
		if !model.ValidID(*id) {
			return 2, errors.New("valid run ID required")
		}
		return 0, tasks.Execute(ctx, s, *id)
	case "status", "ui":
		return a.status(ctx, s, command, args)
	case "logs":
		return a.logs(ctx, s, args)
	case "report", "review":
		return a.report(s, command, args)
	case "cancel":
		f := a.fs("cancel")
		id := f.String("id", "", "task ID")
		f.Bool("json", false, "")
		if e = f.Parse(args); e != nil {
			return 2, e
		}
		if !model.ValidID(*id) {
			return 2, errors.New("valid task ID required")
		}
		if e = tasks.Cancel(s, *id); e != nil {
			return 2, e
		}
		return 0, a.emit(map[string]string{"task": *id, "state": "cancellation_requested", "warning": "does not undo external side effects"})
	case "events":
		f := a.fs("events")
		id := f.String("id", "", "record ID")
		f.Bool("json", false, "")
		if e = f.Parse(args); e != nil {
			return 2, e
		}
		if !model.ValidID(*id) {
			return 2, errors.New("valid record ID required")
		}
		r, e := s.Events(*id)
		if e != nil {
			return 2, e
		}
		return 0, a.emit(r)
	case "export":
		f := a.fs("export")
		kind := f.String("kind", "investigation", "record kind")
		f.Bool("json", false, "")
		if e = f.Parse(args); e != nil {
			return 2, e
		}
		switch *kind {
		case "task", "investigation", "snapshot", "approval", "outcome":
		default:
			return 2, errors.New("unsupported export kind")
		}
		r, e := s.List(*kind, 1000)
		if e != nil {
			return 2, e
		}
		return 0, a.emit(map[string]any{"schema": model.Schema, "kind": *kind, "records": r, "limit": 1000, "note": "bounded export; not a complete backup; session portability is unsupported; artifacts are stored separately"})
	case "outcome":
		return a.outcome(s, args)
	case "publish":
		f := a.fs("publish")
		id := f.String("id", "", "investigation ID")
		to := f.String("to", "", "destination directory outside state")
		f.Bool("json", false, "")
		if e = f.Parse(args); e != nil {
			return 2, e
		}
		if *id == "" || *to == "" {
			return 2, errors.New("publish requires --id and --to")
		}
		out, e := publish.Publish(s, *id, *to)
		if e != nil {
			return 2, e
		}
		return 0, a.emit(map[string]any{"schema": model.Schema, "published": out, "note": "local advisory publisher; not a protected CI gate"})
	default:
		return 2, fmt.Errorf("unknown command %q; run rover help", command)
	}
}
func (a *App) init(ctx context.Context, args []string) (int, error) {
	f := a.fs("init")
	repo := f.String("repo", ".", "repository")
	apply := f.Bool("apply", false, "write config (never overwrite)")
	f.Bool("json", false, "")
	if e := f.Parse(args); e != nil {
		return 2, e
	}
	root, e := source.Discover(ctx, *repo)
	if e != nil {
		return 2, e
	}
	c := config.Suggest(root)
	p := filepath.Join(root, ".rover", "config.json")
	if *apply {
		if e = store.PrivateDir(filepath.Dir(p)); e != nil {
			return 2, e
		}
		b, e := json.MarshalIndent(c, "", "  ")
		if e != nil {
			return 2, e
		}
		file, e := os.OpenFile(p, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if e != nil {
			return 2, fmt.Errorf("configuration not overwritten: %w", e)
		}
		_, we := file.Write(append(b, '\n'))
		se := file.Sync()
		ce := file.Close()
		if we != nil {
			return 2, we
		}
		if se != nil {
			return 2, se
		}
		if ce != nil {
			return 2, ce
		}
	}
	return 0, a.emit(map[string]any{"schema": model.Schema, "path": p, "written": *apply, "config": c, "notes": []string{"No repository commands or installers executed.", "Existing AGENTS.md and CLAUDE.md are unchanged.", "Commit approved configuration before default verification; explicit --config remains advisory.", "Automatic check suggestions currently recognize Go only; other languages use explicit checks."}})
}
func (a *App) inspectVerify(ctx context.Context, s *store.Store, command string, args []string) (int, error) {
	f := a.fs(command)
	repo := f.String("repo", ".", "repository")
	baseRef := f.String("base", "HEAD", "approved baseline Git commit/ref")
	candidateRef := f.String("candidate", "HEAD", "candidate commit/ref")
	worktree := f.Bool("worktree", false, "capture mutable working files using two content reads")
	untracked := f.Bool("include-untracked", false, "explicitly include nonignored untracked files with --worktree")
	jsonOut := f.Bool("json", false, "")
	cfg := f.String("config", "", "explicit user-approved config; otherwise read from base snapshot")
	mode := f.String("mode", "local-advisory", "local-advisory or restricted-docker")
	image := f.String("image", "", "pinned image digest")
	allow := f.Bool("allow-local", false, "authorize code execution with user permissions")
	strategy := f.String("strategy", "", "explicitly approved check-order strategy ID; never selected automatically")
	if e := f.Parse(args); e != nil {
		return 2, e
	}
	if len(f.Args()) > 0 {
		return 2, errors.New("unexpected positional arguments")
	}
	if *untracked && !*worktree {
		return 2, errors.New("--include-untracked requires --worktree")
	}
	if command == "verify" {
		if e := execution.Admit(*mode, *allow, *image); e != nil {
			return 2, e
		}
	}
	base, e := source.Capture(ctx, s, *repo, *baseRef, false)
	if e != nil {
		return 2, e
	}
	if *worktree {
		*candidateRef = "WORKTREE"
	}
	candidate, e := source.Capture(ctx, s, *repo, *candidateRef, *untracked)
	if e != nil {
		return 2, e
	}
	if command == "inspect" {
		in := source.Compare(base, candidate)
		if *jsonOut {
			return 0, a.emit(in)
		}
		fmt.Fprintln(a.Out, "ROVER / CHANGE INSPECTION")
		fmt.Fprintln(a.Out, "Base:", base.ID)
		fmt.Fprintln(a.Out, "Candidate:", candidate.ID)
		for _, ch := range in.Changes {
			fmt.Fprintf(a.Out, "  %-9s %-20s %q\n", ch.Status, ch.Category, ch.Path)
		}
		fmt.Fprintln(a.Out, "Verification not executed. Source files captured:", len(candidate.Files))
		return 0, nil
	}
	c, origin, e := assurance.LoadConfig(s, base, *cfg)
	if e != nil {
		return 2, e
	}
	in, e := assurance.Verify(ctx, s, base, candidate, c, assurance.Options{Mode: *mode, Image: *image, AllowLocal: *allow, PolicySource: origin, Strategy: *strategy})
	if e != nil {
		return 2, e
	}
	if *jsonOut {
		e = a.emit(in)
	} else {
		a.humanReport(in)
	}
	code := DecisionExit(in.Decision)
	return code, e
}

// check is the one-command path for "what changed and does it verify":
// capture base + candidate, compare, run the approved checks, then print the
// decision with the next explicit step. Evidence semantics match verify;
// only the typing is shorter.
func (a *App) check(ctx context.Context, s *store.Store, args []string) (int, error) {
	f := a.fs("check")
	repo := f.String("repo", ".", "repository")
	baseRef := f.String("base", "HEAD", "approved baseline Git commit/ref")
	candidateRef := f.String("candidate", "HEAD", "candidate commit/ref")
	worktree := f.Bool("worktree", false, "capture mutable working files using two content reads")
	untracked := f.Bool("include-untracked", false, "explicitly include nonignored untracked files with --worktree")
	jsonOut := f.Bool("json", false, "")
	cfg := f.String("config", "", "explicit user-approved config; otherwise read from base snapshot")
	mode := f.String("mode", "local-advisory", "local-advisory or restricted-docker")
	image := f.String("image", "", "pinned image digest")
	allow := f.Bool("allow-local", false, "authorize code execution with user permissions")
	strategy := f.String("strategy", "", "explicitly approved check-order strategy ID; never selected automatically")
	if e := f.Parse(args); e != nil {
		return 2, e
	}
	if len(f.Args()) > 0 {
		return 2, errors.New("unexpected positional arguments")
	}
	if *untracked && !*worktree {
		return 2, errors.New("--include-untracked requires --worktree")
	}
	if e := execution.Admit(*mode, *allow, *image); e != nil {
		return 2, e
	}
	base, e := source.Capture(ctx, s, *repo, *baseRef, false)
	if e != nil {
		return 2, e
	}
	if *worktree {
		*candidateRef = "WORKTREE"
	}
	candidate, e := source.Capture(ctx, s, *repo, *candidateRef, *untracked)
	if e != nil {
		return 2, e
	}
	in := source.Compare(base, candidate)
	c, origin, e := assurance.LoadConfig(s, base, *cfg)
	if e != nil {
		return 2, e
	}
	ev, e := assurance.Verify(ctx, s, base, candidate, c, assurance.Options{Mode: *mode, Image: *image, AllowLocal: *allow, PolicySource: origin, Strategy: *strategy})
	if e != nil {
		return 2, e
	}
	next := fmt.Sprintf("next: rover review --id %s --note \"Reviewed\"  |  rover diff --id %s --output change.patch", ev.ID, ev.ID)
	if *jsonOut {
		if e = a.emit(map[string]any{"schema": model.Schema, "inspection": in, "investigation": ev, "next": next}); e != nil {
			return 2, e
		}
	} else {
		fmt.Fprintln(a.Out, "ROVER / CHECK")
		fmt.Fprintln(a.Out, "Base:", base.ID)
		fmt.Fprintln(a.Out, "Candidate:", candidate.ID)
		fmt.Fprintln(a.Out, "Changed files:", len(in.Changes))
		for _, ch := range in.Changes {
			fmt.Fprintf(a.Out, "  %-9s %-20s %q\n", ch.Status, ch.Category, ch.Path)
		}
		a.humanReport(ev)
		fmt.Fprintln(a.Out, next)
	}
	return DecisionExit(ev.Decision), nil
}

// do is the one-command path for "run this task and wait": it submits with
// foreground supervision, so one invocation replaces task run + status/logs
// polling. Exit 0 means the run finished; consult the decision, not the exit.
func (a *App) do(ctx context.Context, s *store.Store, args []string) (int, error) {
	f := a.fs("do")
	file := f.String("file", "", "task contract JSON")
	allow := f.Bool("allow-local", false, "grant local execution")
	key := f.String("key", "", "idempotency key")
	f.Bool("json", false, "")
	if e := f.Parse(args); e != nil {
		return 2, e
	}
	var t model.TaskSpec
	if e := config.Read(*file, &t); e != nil {
		return 2, e
	}
	abs, e := filepath.Abs(*file)
	if e != nil {
		return 2, e
	}
	if !filepath.IsAbs(t.Repository) {
		t.Repository = filepath.Join(filepath.Dir(abs), t.Repository)
	}
	if t.ConfigPath != "" && !filepath.IsAbs(t.ConfigPath) {
		t.ConfigPath = filepath.Join(filepath.Dir(abs), t.ConfigPath)
	}
	if len(*key) > 256 {
		return 2, errors.New("idempotency key too long")
	}
	r, e := tasks.Submit(ctx, s, t, *key, *allow, true)
	if e != nil {
		return 2, e
	}
	if e := a.emit(r); e != nil {
		return 2, e
	}
	fmt.Fprintf(a.Err, "next: rover logs --id %s  |  rover report --id %s\n", r.ID, r.InvestigationID)
	return 0, nil
}
func DecisionExit(d string) int {
	switch d {
	case "ACCEPTED":
		return 0
	case "BLOCKED":
		return 1
	case "REVIEW_REQUIRED":
		return 3
	default:
		return 2
	}
}
func (a *App) humanReport(in model.Investigation) {
	fmt.Fprintln(a.Out, "ROVER / EVIDENCE REPORT")
	fmt.Fprintln(a.Out, "Investigation:", in.ID)
	fmt.Fprintln(a.Out, "Candidate:", in.Candidate)
	fmt.Fprintln(a.Out, "Decision:", in.Decision)
	fmt.Fprintln(a.Out, "Executor:", in.Executor)
	fmt.Fprintln(a.Out, "Trust:", execution.SafeText(in.Trust))
	for _, c := range in.Checks {
		fmt.Fprintf(a.Out, "  %-18s %-14s %s\n", c.ID, c.Outcome, execution.SafeText(c.Meaning))
	}
	for _, f := range in.Findings {
		fmt.Fprintf(a.Out, "  FINDING %-30s %q\n", f.Rule, f.Path)
	}
	for _, u := range in.Unknowns {
		fmt.Fprintln(a.Out, "  UNKNOWN", execution.SafeText(u))
	}
	fmt.Fprintln(a.Out, execution.SafeText(in.DecisionReason))
}
func (a *App) task(ctx context.Context, s *store.Store, args []string) (int, error) {
	if len(args) == 0 || args[0] != "run" {
		return 2, errors.New("usage: rover task run --file TASK.json --allow-local")
	}
	f := a.fs("task run")
	file := f.String("file", "", "task contract JSON")
	allow := f.Bool("allow-local", false, "grant local execution")
	foreground := f.Bool("foreground", false, "supervise in current process")
	key := f.String("key", "", "idempotency key")
	f.Bool("json", false, "")
	if e := f.Parse(args[1:]); e != nil {
		return 2, e
	}
	var t model.TaskSpec
	if e := config.Read(*file, &t); e != nil {
		return 2, e
	}
	abs, e := filepath.Abs(*file)
	if e != nil {
		return 2, e
	}
	if !filepath.IsAbs(t.Repository) {
		t.Repository = filepath.Join(filepath.Dir(abs), t.Repository)
	}
	if t.ConfigPath != "" && !filepath.IsAbs(t.ConfigPath) {
		t.ConfigPath = filepath.Join(filepath.Dir(abs), t.ConfigPath)
	}
	if len(*key) > 256 {
		return 2, errors.New("idempotency key too long")
	}
	r, e := tasks.Submit(ctx, s, t, *key, *allow, *foreground)
	if e != nil {
		return 2, e
	}
	return 0, a.emit(r)
}
func (a *App) status(ctx context.Context, s *store.Store, command string, args []string) (int, error) {
	f := a.fs(command)
	id := f.String("id", "", "task identifier")
	reconcile := f.Bool("reconcile", false, "mark definitely lost stale supervisors")
	watch := f.Bool("watch", false, "refresh dashboard until Ctrl-C")
	j := f.Bool("json", false, "")
	if e := f.Parse(args); e != nil {
		return 2, e
	}
	if *watch && *j {
		return 2, errors.New("--watch and --json are incompatible")
	}
	if *reconcile {
		if e := tasks.Reconcile(s); e != nil {
			return 2, e
		}
	}
	render := func() error {
		if *id != "" {
			if !model.ValidID(*id) {
				return errors.New("valid task ID required")
			}
			r, e := tasks.Get(s, *id)
			if e != nil {
				return e
			}
			return a.emit(r)
		}
		list, e := s.List("task", 100)
		if e != nil {
			return e
		}
		if command == "status" || *j {
			return a.emit(list)
		}
		fmt.Fprintln(a.Out, "ROVER / LOCAL TASKS       advisory alpha — no automatic merge")
		for _, b := range list {
			var r model.TaskRun
			if e = json.Unmarshal(b, &r); e != nil {
				return e
			}
			objective := execution.SafeText(r.Contract.Objective)
			if len(objective) > 72 {
				objective = objective[:72] + "..."
			}
			fmt.Fprintf(a.Out, "%s  %-18s %q\n", r.ID, r.Status, objective)
		}
		if len(list) == 0 {
			fmt.Fprintln(a.Out, "No tasks. Start with: rover task run --file TASK.json --allow-local")
		}
		fmt.Fprintln(a.Out, "Use status --id, logs --id, report --id, and cancel --id. Watch exits with Ctrl-C.")
		return nil
	}
	if !*watch {
		return 0, render()
	}
	// No full terminal emulation: dashboard is a thin, optionally refreshing view.
	tty := false
	if out, ok := a.Out.(*os.File); ok {
		st, e := out.Stat()
		tty = e == nil && st.Mode()&os.ModeCharDevice != 0
	}
	if !tty {
		return 2, errors.New("--watch requires a terminal; use status --json for agents")
	}
	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()
	for {
		fmt.Fprint(a.Out, "\x1b[2J\x1b[H")
		if e := render(); e != nil {
			return 2, e
		}
		select {
		case <-ctx.Done():
			return 0, nil
		case <-ticker.C:
		}
	}
}
func (a *App) logs(ctx context.Context, s *store.Store, args []string) (int, error) {
	f := a.fs("logs")
	id := f.String("id", "", "task identifier")
	follow := f.Bool("follow", false, "poll bounded headless output")
	stderr := f.Bool("stderr", false, "read stderr")
	j := f.Bool("json", false, "")
	if e := f.Parse(args); e != nil {
		return 2, e
	}
	if *j && *follow {
		return 2, errors.New("JSON follow is not supported")
	}
	if !model.ValidID(*id) {
		return 2, errors.New("valid task ID required")
	}
	file := "stdout.log"
	if *stderr {
		file = "stderr.log"
	}
	offset := 0
	for {
		r, e := tasks.Get(s, *id)
		if e != nil {
			return 2, e
		}
		p := filepath.Join(s.Root, "tasks", *id, "output", file)
		b, e := store.BoundedFile(p, execution.OutputLimit)
		if e != nil && !os.IsNotExist(e) {
			return 2, e
		}
		if *j {
			return 0, a.emit(map[string]any{"task": *id, "stream": file, "text": string(b), "status": r.Status, "scope": "headless output; not a complete agent action audit"})
		}
		if len(b) > offset {
			fmt.Fprint(a.Out, execution.SafeText(string(b[offset:])))
			offset = len(b)
		}
		if !*follow || model.Terminal(r.Status) {
			return 0, nil
		}
		select {
		case <-ctx.Done():
			return 0, nil
		case <-time.After(200 * time.Millisecond):
		}
	}
}
func (a *App) report(s *store.Store, command string, args []string) (int, error) {
	f := a.fs(command)
	id := f.String("id", "", "investigation identifier")
	note := f.String("note", "", "local review note")
	j := f.Bool("json", false, "")
	if e := f.Parse(args); e != nil {
		return 2, e
	}
	if !model.ValidID(*id) {
		return 2, errors.New("valid investigation ID required")
	}
	if command == "review" {
		r, e := assurance.Review(s, *id, *note)
		if e != nil {
			return 2, e
		}
		return 0, a.emit(r)
	}
	var in model.Investigation
	if e := s.Get("investigation", *id, &in); e != nil {
		return 2, e
	}
	if *j {
		return 0, a.emit(in)
	}
	a.humanReport(in)
	return 0, nil
}
func (a *App) outcome(s *store.Store, args []string) (int, error) {
	f := a.fs("outcome")
	id := f.String("id", "", "investigation identifier")
	label := f.String("label", "", "human-accepted, human-rejected, incident-confirmed, or reverted")
	note := f.String("note", "", "observation and attribution note")
	f.Bool("json", false, "")
	if e := f.Parse(args); e != nil {
		return 2, e
	}
	switch *label {
	case "human-accepted", "human-rejected", "incident-confirmed", "reverted":
	default:
		return 2, errors.New("unsupported outcome label")
	}
	if strings.TrimSpace(*note) == "" {
		return 2, errors.New("outcome note required; temporal proximity is not causal evidence")
	}
	if !model.ValidID(*id) {
		return 2, errors.New("valid investigation ID required")
	}
	var in model.Investigation
	if e := s.Get("investigation", *id, &in); e != nil {
		return 2, e
	}
	oid := model.ID("outcome")
	record := map[string]string{"schema": model.Schema, "id": oid, "investigation": in.ID, "candidate": in.Candidate, "label": *label, "note": *note, "source": "explicit_local_user_assertion", "at": model.Now()}
	if e := s.Put("outcome", oid, record, "outcome.recorded"); e != nil {
		return 2, e
	}
	return 0, a.emit(record)
}
