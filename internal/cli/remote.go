package cli

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"time"

	"github.com/GrayCodeAI/rover/internal/attestation"
	"github.com/GrayCodeAI/rover/internal/mcp"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/store"
)

type node struct {
	Schema    string `json:"schema"`
	ID        string `json:"id"`
	Endpoint  string `json:"endpoint"`
	TokenFile string `json:"token_file"`
	CAFile    string `json:"ca_file,omitempty"`
	CreatedAt string `json:"created_at"`
}

func (a *extendedApp) remoteCommand(ctx context.Context, args []string) int {
	if len(args) == 0 {
		return a.fail(errors.New("remote tools|call|node-add|node-list|node-delete"))
	}
	f := a.flags("remote " + args[0])
	endpoint := f.String("endpoint", "", "Rover HTTP MCP endpoint")
	token := f.String("token-file", "", "private bearer-token file")
	ca := f.String("ca-file", "", "explicit TLS trust roots")
	name := f.String("node", "", "saved endpoint name")
	tool := f.String("tool", "", "tool to call")
	file := f.String("arguments", "", "JSON object file; defaults to {}")
	timeout := f.Duration("timeout", 15*time.Minute, "whole operation timeout")
	if e := parse(f, args[1:]); e != nil {
		return a.fail(e)
	}
	if *timeout <= 0 || *timeout > 24*time.Hour {
		return a.fail(errors.New("timeout must be between zero and 24h"))
	}
	return a.withStore(func(s *store.Store) int {
		n := node{Schema: model.Schema, ID: *name, Endpoint: *endpoint, TokenFile: *token, CAFile: *ca, CreatedAt: model.Now()}
		if args[0] == "node-list" {
			rows, e := s.ListAll("node", 10000)
			if e != nil {
				return a.fail(e)
			}
			return a.emit(rows)
		}
		if args[0] == "node-delete" {
			if e := s.Delete("node", *name, "node.removed"); e != nil {
				return a.fail(e)
			}
			return a.emit(map[string]string{"removed": *name, "scope": "local connection reference only; does not delete a server or revoke its token"})
		}
		if args[0] != "node-add" && *name != "" {
			if *endpoint != "" || *token != "" || *ca != "" {
				return a.fail(errors.New("saved node cannot be mixed with endpoint overrides"))
			}
			if e := s.Get("node", *name, &n); e != nil {
				return a.fail(e)
			}
		}
		secret, e := attestation.ReadKey(n.TokenFile, true)
		if e != nil {
			return a.fail(e)
		}
		client, e := mcp.NewRemote(n.Endpoint, strings.TrimSpace(string(secret)), n.CAFile)
		if e != nil {
			return a.fail(e)
		}
		defer client.Close()
		c, cancel := context.WithTimeout(ctx, *timeout)
		defer cancel()
		switch args[0] {
		case "node-add":
			if !model.ValidID(n.ID) {
				return a.fail(errors.New("valid --node ID required"))
			}
			if e = client.Initialize(c); e != nil {
				return a.fail(e)
			}
			n.TokenFile, e = filepath.Abs(n.TokenFile)
			if e != nil {
				return a.fail(e)
			}
			if n.CAFile != "" {
				n.CAFile, e = filepath.Abs(n.CAFile)
				if e != nil {
					return a.fail(e)
				}
			}
			var prior node
			if e = s.Get("node", n.ID, &prior); !errors.Is(e, store.ErrNotFound) {
				return a.fail(errors.New("node exists or cannot be inspected; no overwrite"))
			}
			if e = s.Put("node", n.ID, n, "node.added"); e != nil {
				return a.fail(e)
			}
			return a.emit(n)
		case "tools":
			b, e := client.Tools(c)
			if e != nil {
				return a.fail(e)
			}
			return a.emit(b)
		case "call":
			raw := []byte(`{}`)
			if *file != "" {
				raw, e = readBoundedFile(*file, mcp.MaxMessage)
				if e != nil {
					return a.fail(e)
				}
			}
			r, e := client.Call(c, *tool, json.RawMessage(raw))
			if e != nil {
				return a.fail(e)
			}
			if code := a.emit(r); code != 0 {
				return code
			}
			if r.IsError {
				return 2
			}
			return 0
		default:
			return a.fail(errors.New("unknown remote operation"))
		}
	})
}
