package cli

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"github.com/GrayCodeAI/rover/internal/agents"
	"github.com/GrayCodeAI/rover/internal/assurance"
	"github.com/GrayCodeAI/rover/internal/config"
	"github.com/GrayCodeAI/rover/internal/contextstore"
	"github.com/GrayCodeAI/rover/internal/execution"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/store"
	"github.com/GrayCodeAI/rover/internal/tasks"
	"github.com/GrayCodeAI/rover/internal/workflow"
	"os"
	"path/filepath"
	"strings"
	"time"
)

type stringList []string

func (s *stringList) String() string     { return strings.Join(*s, ",") }
func (s *stringList) Set(v string) error { *s = append(*s, v); return nil }

type subjectFlags struct {
	repo, base, baseID, candidate, config, mode, image *string
	worktree, untracked, allow                         *bool
}

func subjectOptions(f *flag.FlagSet) subjectFlags {
	return subjectFlags{repo: f.String("repo", ".", "repository"), base: f.String("base", "HEAD", "approved base reference"), baseID: f.String("base-snapshot", "", "retained baseline identity"), candidate: f.String("candidate", "", "retained candidate identity"), config: f.String("config", "", "explicit approved config"), mode: f.String("mode", "local-advisory", "executor mode"), image: f.String("image", "", "pinned Docker image"), worktree: f.Bool("worktree", false, "capture mutable working tree"), untracked: f.Bool("include-untracked", false, "include eligible untracked files"), allow: f.Bool("allow-local", false, "explicitly permit user-level execution")}
}
func (f subjectFlags) resolve(ctx context.Context, s *store.Store) (model.Snapshot, model.Snapshot, model.Config, assurance.Options, error) {
	var base, cand model.Snapshot
	var cfg model.Config
	o := assurance.Options{Mode: *f.mode, AllowLocal: *f.allow, Image: *f.image}
	var e error
	if *f.baseID != "" {
		base, e = source.Load(s, *f.baseID)
	} else {
		base, e = source.Capture(ctx, s, *f.repo, *f.base, false)
	}
	if e != nil {
		return base, cand, cfg, o, e
	}
	if *f.candidate != "" {
		if *f.worktree {
			return base, cand, cfg, o, errors.New("candidate and worktree are mutually exclusive")
		}
		cand, e = source.Load(s, *f.candidate)
	} else {
		ref := "HEAD"
		if *f.worktree {
			ref = "WORKTREE"
		}
		cand, e = source.Capture(ctx, s, *f.repo, ref, *f.untracked)
	}
	if e != nil {
		return base, cand, cfg, o, e
	}
	if cand.Repository != base.Repository {
		return base, cand, cfg, o, errors.New("candidate repository mismatch")
	}
	cfg, o.PolicySource, e = assurance.LoadConfig(s, base, *f.config)
	return base, cand, cfg, o, e
}
func (a *extendedApp) advanced(ctx context.Context, args []string) int {
	switch args[0] {
	case "agent":
		return a.agentCommand(args[1:])
	case "attach":
		return a.attachCommand(ctx, args[1:])
	case "tui":
		return a.withStore(func(s *store.Store) int { return a.tui(ctx, s) })
	case "workflow", "__workflow":
		return a.workflowCommand(ctx, args)
	case "prove", "mutate":
		return a.probeCommand(ctx, args)
	case "replay":
		return a.replayCommand(ctx, args[1:])
	case "diff":
		return a.diffCommand(ctx, args[1:])
	case "context", "memory":
		return a.contextCommand(ctx, args)
	case "mcp", "serve":
		return a.serverCommand(ctx, args)
	case "grant":
		return a.grantCommand(ctx, args[1:])
	case "backup", "restore", "backup-check":
		return a.archiveCommand(ctx, args)
	case "attest":
		return a.attestCommand(ctx, args[1:])
	case "integrate":
		return a.integrateCommand(ctx, args[1:])
	case "learn":
		return a.learnCommand(ctx, args[1:])
	case "remote":
		return a.remoteCommand(ctx, args[1:])
	case "limits":
		return a.limitsCommand(args[1:])
	default:
		return a.fail(errors.New("unknown extension command"))
	}
}
func (a *extendedApp) agentCommand(args []string) int {
	if len(args) == 0 || args[0] == "list" {
		return a.emit(map[string]any{"schema": model.Schema, "adapters": agents.List()})
	}
	if args[0] == "capabilities" && len(args) == 2 {
		for _, d := range agents.List() {
			if d.Name == args[1] {
				return a.emit(d)
			}
		}
	}
	return a.fail(errors.New("agent list | agent capabilities <adapter>"))
}
func (a *extendedApp) attachCommand(ctx context.Context, args []string) int {
	f := a.flags("attach")
	id := f.String("id", "", "task identity")
	if e := parse(f, args); e != nil {
		return a.fail(e)
	}
	return a.withStore(func(s *store.Store) int {
		t, e := tasks.Get(s, *id)
		if e != nil {
			return a.fail(e)
		}
		if !t.Contract.Interactive || t.Socket == "" {
			return a.fail(errors.New("task has no interactive terminal"))
		}
		if e = execution.Attach(ctx, t.Socket, os.Stdin, os.Stdout); e != nil {
			return a.fail(e)
		}
		return 0
	})
}
func (a *extendedApp) workflowCommand(ctx context.Context, args []string) int {
	if args[0] == "__workflow" {
		f := a.flags("__workflow")
		id := f.String("id", "", "workflow")
		if e := parse(f, args[1:]); e != nil {
			return a.fail(e)
		}
		return a.withStore(func(s *store.Store) int {
			if e := workflow.Execute(ctx, s, *id); e != nil {
				return a.fail(e)
			}
			return 0
		})
	}
	if len(args) < 2 {
		return a.fail(errors.New("workflow run | status | cancel"))
	}
	f := a.flags("workflow " + args[1])
	file := f.String("file", "", "strict workflow JSON")
	id := f.String("id", "", "workflow identity")
	key := f.String("key", "", "idempotency key")
	allow := f.Bool("allow-local", false, "permit local user-level execution")
	foreground := f.Bool("foreground", false, "run supervisor in foreground")
	if e := parse(f, args[2:]); e != nil {
		return a.fail(e)
	}
	return a.withStore(func(s *store.Store) int {
		switch args[1] {
		case "run":
			var sp workflow.Spec
			if e := config.Read(*file, &sp); e != nil {
				return a.fail(e)
			}
			dir, e := filepath.Abs(filepath.Dir(*file))
			if e != nil {
				return a.fail(e)
			}
			if !filepath.IsAbs(sp.Repository) {
				sp.Repository = filepath.Join(dir, sp.Repository)
			}
			if sp.ConfigPath != "" && !filepath.IsAbs(sp.ConfigPath) {
				sp.ConfigPath = filepath.Join(dir, sp.ConfigPath)
			}
			r, e := workflow.Submit(ctx, s, sp, *key, *allow, *foreground)
			if e != nil {
				return a.fail(e)
			}
			return a.emit(r)
		case "status":
			if e := workflow.Reconcile(s); e != nil {
				return a.fail(e)
			}
			if *id != "" {
				r, e := workflow.Get(s, *id)
				if e != nil {
					return a.fail(e)
				}
				return a.emit(r)
			}
			r, e := s.List("workflow", 200)
			if e != nil {
				return a.fail(e)
			}
			return a.emit(r)
		case "cancel":
			if e := workflow.Cancel(s, *id); e != nil {
				return a.fail(e)
			}
			r, e := workflow.Get(s, *id)
			if e != nil {
				return a.fail(e)
			}
			return a.emit(r)
		default:
			return a.fail(errors.New("unknown workflow subcommand"))
		}
	})
}
func (a *extendedApp) probeCommand(ctx context.Context, args []string) int {
	name := args[0]
	rest := args[1:]
	if name == "prove" {
		if len(rest) == 0 || rest[0] != "regression" {
			return a.fail(errors.New("prove regression --file <spec.json>; no universal correctness proof"))
		}
		rest = rest[1:]
	}
	f := a.flags(name)
	sf := subjectOptions(f)
	file := f.String("file", "", "explicit approved probe specification")
	if e := parse(f, rest); e != nil {
		return a.fail(e)
	}
	return a.withStore(func(s *store.Store) int {
		base, cand, cfg, o, e := sf.resolve(ctx, s)
		if e != nil {
			return a.fail(e)
		}
		if name == "prove" {
			var sp assurance.RegressionSpec
			if e = config.Read(*file, &sp); e != nil {
				return a.fail(e)
			}
			r, e := assurance.ProveRegression(ctx, s, base, cand, cfg, sp, o)
			if e != nil {
				return a.fail(e)
			}
			a.emit(r)
			if r.Assessment == "SUPPORTED_WITHIN_SCOPE" {
				return 0
			}
			return 2
		}
		var sp assurance.MutationSpec
		if e = config.Read(*file, &sp); e != nil {
			return a.fail(e)
		}
		r, e := assurance.Mutate(ctx, s, base, cand, cfg, sp, o)
		if e != nil {
			return a.fail(e)
		}
		return a.emit(r)
	})
}
func (a *extendedApp) replayCommand(ctx context.Context, args []string) int {
	f := a.flags("replay")
	id := f.String("id", "", "previous investigation")
	allow := f.Bool("allow-local", false, "permit local commands")
	mode := f.String("mode", "local-advisory", "mode")
	image := f.String("image", "", "pinned image")
	if e := parse(f, args); e != nil {
		return a.fail(e)
	}
	return a.withStore(func(s *store.Store) int {
		r, e := assurance.Replay(ctx, s, *id, assurance.Options{Mode: *mode, AllowLocal: *allow, Image: *image})
		if e != nil {
			return a.fail(e)
		}
		a.emit(r)
		return assurance.ExitCode(r.Decision)
	})
}
func (a *extendedApp) diffCommand(ctx context.Context, args []string) int {
	f := a.flags("diff")
	base := f.String("base", "", "baseline snapshot")
	candidate := f.String("candidate", "", "candidate snapshot")
	id := f.String("id", "", "investigation id instead")
	out := f.String("output", "", "write applicable patch; refuses overwrite")
	j := a.jsonMode
	if e := parse(f, args); e != nil {
		return a.fail(e)
	}
	return a.withStore(func(s *store.Store) int {
		if *id != "" {
			if !model.ValidID(*id) {
				return a.fail(errors.New("valid investigation ID required"))
			}
			var in model.Investigation
			if e := s.Get("investigation", *id, &in); e != nil {
				return a.fail(e)
			}
			*base = in.Base
			*candidate = in.Candidate
		}
		if !model.ValidID(*base) || !model.ValidID(*candidate) {
			return a.fail(errors.New("valid snapshot IDs required"))
		}
		b, e := source.Load(s, *base)
		if e != nil {
			return a.fail(e)
		}
		c, e := source.Load(s, *candidate)
		if e != nil {
			return a.fail(e)
		}
		patch, e := source.Diff(ctx, s, b, c)
		if e != nil {
			return a.fail(e)
		}
		if *out != "" {
			f, e := os.OpenFile(*out, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
			if e != nil {
				return a.fail(e)
			}
			_, we := f.Write(patch)
			ce := f.Close()
			if we != nil {
				return a.fail(we)
			}
			if ce != nil {
				return a.fail(ce)
			}
			return a.emit(map[string]any{"output": *out, "sha256": model.Digest(patch), "candidate": c.ID})
		}
		if j {
			return a.emit(map[string]any{"base": b.ID, "candidate": c.ID, "patch": string(patch)})
		}
		if len(patch) > 1<<20 {
			return a.fail(errors.New("patch exceeds 1MiB display budget; use diff --output"))
		}
		fmt.Fprint(a.Out, execution.SafeText(string(patch)))
		return 0
	})
}
func (a *extendedApp) contextCommand(ctx context.Context, args []string) int {
	if len(args) < 2 {
		return a.fail(errors.New("context search|bundle, memory put|list|delete"))
	}
	f := a.flags(strings.Join(args[:2], " "))
	repo := f.String("repo", ".", "repository")
	snapshot := f.String("snapshot", "", "retained snapshot")
	query := f.String("query", "", "literal query")
	limit := f.Int("limit", 40, "match bound")
	text := f.String("text", "", "note contents")
	origin := f.String("origin", "human", "human or agent assertion")
	ttl := f.Duration("ttl", 30*24*time.Hour, "note expiry")
	id := f.String("id", "", "note identity")
	var paths stringList
	f.Var(&paths, "path", "explicit context path, repeatable")
	if e := parse(f, args[2:]); e != nil {
		return a.fail(e)
	}
	return a.withStore(func(s *store.Store) int {
		var snap model.Snapshot
		var e error
		if *snapshot != "" {
			snap, e = source.Load(s, *snapshot)
		} else {
			snap, e = source.Capture(ctx, s, *repo, "HEAD", false)
		}
		if e != nil {
			return a.fail(e)
		}
		if args[0] == "context" {
			switch args[1] {
			case "search":
				r, e := contextstore.Search(ctx, s, snap, *query, *limit)
				if e != nil {
					return a.fail(e)
				}
				return a.emit(r)
			case "bundle":
				r, e := contextstore.Build(s, snap, paths)
				if e != nil {
					return a.fail(e)
				}
				return a.emit(r)
			}
		} else {
			switch args[1] {
			case "put":
				r, e := contextstore.PutNote(s, snap, *text, *origin, *ttl)
				if e != nil {
					return a.fail(e)
				}
				return a.emit(r)
			case "list":
				r, e := contextstore.Notes(s, snap)
				if e != nil {
					return a.fail(e)
				}
				return a.emit(r)
			case "delete":
				var n contextstore.Note
				if e = s.Get("memory", *id, &n); e != nil {
					return a.fail(errors.New("memory record not available"))
				}
				if n.Project != model.Digest([]byte(snap.Repository)) {
					return a.fail(errors.New("memory record not available"))
				}
				if e = s.Delete("memory", *id, "memory.deleted"); e != nil {
					return a.fail(e)
				}
				return a.emit(map[string]any{"deleted": *id, "scope": "active retrieval; prior event/backup history is retained"})
			}
		}
		return a.fail(errors.New("unknown context/memory command"))
	})
}
func (a *extendedApp) limitsCommand(args []string) int {
	f := a.flags("limits")
	max := f.Int("max-agents", 0, "set concurrent task capacity 1..128")
	if e := parse(f, args); e != nil {
		return a.fail(e)
	}
	return a.withStore(func(s *store.Store) int {
		if *max != 0 {
			if *max < 1 || *max > 128 {
				return a.fail(errors.New("max-agents outside 1..128"))
			}
			if e := s.Put("settings", "runtime", map[string]int{"max_agents": *max}, "runtime.capacity_changed"); e != nil {
				return a.fail(e)
			}
		}
		n, e := s.Capacity()
		if e != nil {
			return a.fail(e)
		}
		leases, e := s.ListAll("lease", 10000)
		if e != nil {
			return a.fail(e)
		}
		return a.emit(map[string]any{"max_agents": n, "leases": leases, "semantics": "coordination, not OS isolation; lower limit does not kill admitted work"})
	})
}

// Keep the import used for typed record decoding as command families expand.
var _ = json.RawMessage{}
