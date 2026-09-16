package cli

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"github.com/GrayCodeAI/rover/internal/access"
	"github.com/GrayCodeAI/rover/internal/assurance"
	"github.com/GrayCodeAI/rover/internal/mcp"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/service"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/store"
)

func (a *extendedApp) serverCommand(ctx context.Context, args []string) int {
	cmd := args[0]
	f := a.flags(cmd)
	repo := f.String("repo", ".", "single authorized project")
	base := f.String("base", "HEAD", "approved immutable baseline resolved at startup")
	enable := f.Bool("enable-execution", false, "expose task/verification tools; otherwise read-only")
	allow := f.Bool("allow-local", false, "grant user-level command execution; not a sandbox")
	mode := f.String("mode", "local-advisory", "verification executor")
	image := f.String("image", "", "digest-pinned Docker image")
	listen := f.String("listen", "127.0.0.1:0", "HTTP address; non-loopback requires TLS")
	cert := f.String("tls-cert", "", "TLS certificate")
	key := f.String("tls-key", "", "TLS key")
	ready := f.String("ready-file", "", "write new JSON address file; never overwrite")
	var pass stringList
	f.Var(&pass, "pass-env", "explicit execution environment grant (repeatable)")
	if e := parse(f, args[1:]); e != nil {
		return a.fail(e)
	}
	return a.withStore(func(s *store.Store) int {
		sv, e := service.New(ctx, s, *repo, *base, assurance.Options{Mode: *mode, AllowLocal: *allow, Image: *image}, *enable)
		if e != nil {
			return a.fail(e)
		}
		sv.PassEnv = pass
		if cmd == "mcp" {
			return a.fail(mcp.ServeStdio(ctx, sv, os.Stdin, a.Out))
		}
		c, cancel := context.WithCancel(ctx)
		defer cancel()
		var readyErr error
		e = mcp.RunHTTP(c, sv, *listen, *cert, *key, func(addr string) error {
			scheme := "http"
			if *cert != "" {
				scheme = "https"
			}
			v := map[string]string{"schema": model.Schema, "address": addr, "endpoint": scheme + "://" + addr + "/mcp", "protocol": "2025-11-25", "project": sv.Repository, "baseline": sv.Base.ID}
			b, _ := json.MarshalIndent(v, "", "  ")
			if *ready != "" {
				readyErr = writeNew(*ready, append(b, '\n'), 0600)
				if readyErr != nil {
					cancel()
					return readyErr
				}
			}
			fmt.Fprintln(a.Err, string(b))
			return nil
		})
		if readyErr != nil {
			return a.fail(readyErr)
		}
		return a.fail(e)
	})
}
func writeNew(path string, b []byte, mode os.FileMode) error {
	if path == "" {
		return errors.New("output path required")
	}
	abs, e := filepath.Abs(path)
	if e != nil {
		return e
	}
	// Never chmod arbitrary existing parent directories or follow a final symlink.
	parent, e := filepath.EvalSymlinks(filepath.Dir(abs))
	if e != nil {
		return e
	}
	if parent != filepath.Dir(abs) {
		return errors.New("symlink output parent unsupported")
	}
	f, e := os.OpenFile(abs, os.O_CREATE|os.O_EXCL|os.O_WRONLY, mode)
	if e != nil {
		return e
	}
	ok := false
	defer func() {
		f.Close()
		if !ok {
			os.Remove(abs)
		}
	}()
	if _, e = f.Write(b); e != nil {
		return e
	}
	if e = f.Sync(); e != nil {
		return e
	}
	if e = f.Close(); e != nil {
		return e
	}
	ok = true
	return nil
}
func (a *extendedApp) grantCommand(ctx context.Context, args []string) int {
	if len(args) == 0 {
		return a.fail(errors.New("grant create|list|revoke"))
	}
	f := a.flags("grant " + args[0])
	repo := f.String("repo", ".", "project")
	id := f.String("id", "", "grant ID")
	note := f.String("note", "", "operator reason")
	ttl := f.Duration("ttl", time.Hour, "expiry 1m..720h")
	var tools stringList
	f.Var(&tools, "tool", "allowed tool name (repeatable)")
	if e := parse(f, args[1:]); e != nil {
		return a.fail(e)
	}
	return a.withStore(func(s *store.Store) int {
		project, e := source.Discover(ctx, *repo)
		if e != nil {
			return a.fail(e)
		}
		switch args[0] {
		case "create":
			known := map[string]bool{}
			for _, name := range service.AllNames() {
				known[name] = true
			}
			for _, name := range tools {
				if !known[name] {
					return a.fail(fmt.Errorf("unknown tool %s", name))
				}
			}
			g, secret, e := access.Issue(s, project, tools, *note, *ttl)
			if e != nil {
				return a.fail(e)
			}
			return a.emit(map[string]any{"grant": g, "token": secret, "warning": "Token displayed once. This is a project/tool-scoped local operator grant, not organization identity federation."})
		case "list":
			rows, e := s.ListAll("grant", 100000)
			if e != nil {
				return a.fail(e)
			}
			out := []access.Grant{}
			for _, b := range rows {
				var g access.Grant
				if e = json.Unmarshal(b, &g); e != nil {
					return a.fail(e)
				}
				if g.Project == model.Digest([]byte(project)) {
					out = append(out, g)
				}
			}
			return a.emit(out)
		case "revoke":
			var g access.Grant
			if e = s.Get("grant", *id, &g); e != nil {
				return a.fail(e)
			}
			if g.Project != model.Digest([]byte(project)) {
				return a.fail(errors.New("grant belongs to a different project"))
			}
			if e = access.Revoke(s, *id); e != nil {
				return a.fail(e)
			}
			return a.emit(map[string]string{"id": *id, "status": "revoked", "scope": "new requests denied; already executed side effects are not undone"})
		default:
			return a.fail(errors.New("grant create|list|revoke"))
		}
	})
}
