package cli

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"time"

	"github.com/GrayCodeAI/rover/internal/archive"
	"github.com/GrayCodeAI/rover/internal/assurance"
	"github.com/GrayCodeAI/rover/internal/attestation"
	"github.com/GrayCodeAI/rover/internal/integration"
	"github.com/GrayCodeAI/rover/internal/learning"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/store"
	"github.com/GrayCodeAI/rover/internal/wire"
)

func (a *extendedApp) archiveCommand(ctx context.Context, args []string) int {
	cmd := args[0]
	f := a.flags(cmd)
	from := f.String("from", "", "backup directory")
	to := f.String("to", "", "new destination directory (must not exist)")
	if e := parse(f, args[1:]); e != nil {
		return a.fail(e)
	}
	if cmd == "restore" {
		m, e := archive.Restore(ctx, *from, *to)
		if e != nil {
			return a.fail(e)
		}
		return a.emit(map[string]any{"restored": m, "state": *to, "warning": "Active executions marked LOST and all API grants revoked. No process or original uncommitted workspace is restored."})
	}
	if cmd == "backup-check" {
		m, e := archive.Verify(*from)
		if e != nil {
			return a.fail(e)
		}
		return a.emit(m)
	}
	return a.withStore(func(s *store.Store) int {
		c, cancel := context.WithTimeout(ctx, 30*time.Second)
		defer cancel()
		m, e := archive.Backup(c, s, *to)
		if e != nil {
			return a.fail(e)
		}
		return a.emit(m)
	})
}
func (a *extendedApp) attestCommand(ctx context.Context, args []string) int {
	if len(args) == 0 {
		return a.fail(errors.New("attest keygen|sign|verify"))
	}
	f := a.flags("attest " + args[0])
	key := f.String("key", "", "private key file")
	public := f.String("public-key", "", "expected public key file")
	id := f.String("id", "", "investigation ID")
	out := f.String("out", "", "new envelope path")
	file := f.String("file", "", "signed envelope")
	if e := parse(f, args[1:]); e != nil {
		return a.fail(e)
	}
	switch args[0] {
	case "keygen":
		if *key == "" || *public == "" || *key == *public {
			return a.fail(errors.New("distinct new --key and --public-key required"))
		}
		for _, p := range []string{*key, *public} {
			if _, e := os.Lstat(p); !errors.Is(e, os.ErrNotExist) {
				return a.fail(fmt.Errorf("key path already exists or cannot be inspected: %s", p))
			}
		}
		pr, pu, e := attestation.Keypair()
		if e != nil {
			return a.fail(e)
		}
		if e = writeNew(*key, pr, 0600); e != nil {
			return a.fail(e)
		}
		if e = writeNew(*public, pu, 0644); e != nil {
			return a.fail(e)
		}
		return a.emit(map[string]string{"private_key": *key, "public_key": *public, "warning": "Establish public-key trust out of band. Local signing is not independent verification."})
	case "verify":
		b, e := attestation.ReadKey(*public, false)
		if e != nil {
			return a.fail(e)
		}
		pk, e := attestation.Public(b)
		if e != nil {
			return a.fail(e)
		}
		raw, e := readBoundedFile(*file, 1<<20)
		if e != nil {
			return a.fail(e)
		}
		var env attestation.Envelope
		if e = wire.Decode(raw, &env); e != nil {
			return a.fail(e)
		}
		p, e := attestation.Verify(env, pk)
		if e != nil {
			return a.fail(e)
		}
		return a.emit(map[string]any{"signature_valid": true, "payload": p, "scope": "Authenticates signed bytes under the explicitly supplied key, not software correctness or protected execution."})
	case "sign":
		return a.withStore(func(s *store.Store) int {
			b, e := attestation.ReadKey(*key, true)
			if e != nil {
				return a.fail(e)
			}
			pk, e := attestation.Private(b)
			if e != nil {
				return a.fail(e)
			}
			env, e := attestation.Sign(s, *id, pk)
			if e != nil {
				return a.fail(e)
			}
			b, e = json.MarshalIndent(env, "", "  ")
			if e != nil {
				return a.fail(e)
			}
			if *out != "" {
				if e = writeNew(*out, append(b, '\n'), 0600); e != nil {
					return a.fail(e)
				}
			}
			return a.emit(env)
		})
	default:
		return a.fail(errors.New("attest keygen|sign|verify"))
	}
}
func readBoundedFile(path string, n int64) ([]byte, error) {
	st, e := os.Lstat(path)
	if e != nil {
		return nil, e
	}
	if !st.Mode().IsRegular() || st.Size() > n {
		return nil, errors.New("bounded regular file required")
	}
	f, e := os.Open(path)
	if e != nil {
		return nil, e
	}
	defer f.Close()
	return readBounded(f, n)
}
func (a *extendedApp) integrateCommand(ctx context.Context, args []string) int {
	f := a.flags("integrate")
	repo := f.String("repo", ".", "project")
	agent := f.String("agent", "generic", "generic/codex/opencode/claude/gemini")
	apply := f.Bool("apply", false, "authorize previewed guidance edit")
	expected := f.String("expected-before", "", "optional preview content digest")
	undo := f.String("undo", "", "undo receipt if file unchanged")
	if e := parse(f, args); e != nil {
		return a.fail(e)
	}
	return a.withStore(func(s *store.Store) int {
		if *undo != "" {
			r, e := integration.Undo(s, *undo)
			if e != nil {
				return a.fail(e)
			}
			return a.emit(r)
		}
		p, e := integration.Preview(ctx, *repo, *agent)
		if e != nil {
			return a.fail(e)
		}
		if !*apply {
			return a.emit(p)
		}
		if *expected != "" && *expected != p.Before {
			return a.fail(errors.New("preview digest changed"))
		}
		if !p.Changed {
			return a.emit(p)
		}
		r, e := integration.Apply(ctx, s, p)
		if e != nil {
			return a.fail(e)
		}
		return a.emit(r)
	})
}
func (a *extendedApp) learnCommand(ctx context.Context, args []string) int {
	if len(args) == 0 {
		return a.fail(errors.New("learn recommend|dataset|evaluate|promote|show|revoke"))
	}
	f := a.flags("learn " + args[0])
	repo := f.String("repo", ".", "project")
	base := f.String("base", "HEAD", "approved baseline")
	file := f.String("file", "", "dataset JSON")
	id := f.String("id", "", "record ID")
	dataset := f.String("dataset", "", "held-out dataset ID")
	note := f.String("note", "", "explicit local promotion review")
	if e := parse(f, args[1:]); e != nil {
		return a.fail(e)
	}
	return a.withStore(func(s *store.Store) int {
		switch args[0] {
		case "recommend":
			b, e := source.Capture(ctx, s, *repo, *base, false)
			if e != nil {
				return a.fail(e)
			}
			c, _, e := assurance.LoadConfig(s, b, "")
			if e != nil {
				return a.fail(e)
			}
			p, e := learning.Suggest(s, b.Repository, c)
			if e != nil {
				return a.fail(e)
			}
			return a.emit(p)
		case "dataset":
			var d learning.Dataset
			raw, e := readBoundedFile(*file, 1<<20)
			if e != nil {
				return a.fail(e)
			}
			if e = wire.Decode(raw, &d); e != nil {
				return a.fail(e)
			}
			d, e = learning.Import(s, d)
			if e != nil {
				return a.fail(e)
			}
			return a.emit(d)
		case "evaluate":
			e, err := learning.Evaluate(s, *id, *dataset)
			if err != nil {
				return a.fail(err)
			}
			return a.emit(e)
		case "promote":
			p, e := learning.Promote(s, *id, *note)
			if e != nil {
				return a.fail(e)
			}
			return a.emit(p)
		case "show":
			v, e := learning.Explain(s, *id)
			if e != nil {
				return a.fail(e)
			}
			return a.emit(v)
		case "revoke":
			var p learning.Promotion
			if e := s.Get("strategy", *id, &p); e != nil {
				return a.fail(e)
			}
			p.Revoked = true
			if e := s.Put("strategy", p.ID, p, "learning.revoked"); e != nil {
				return a.fail(e)
			}
			return a.emit(p)
		default:
			return a.fail(errors.New("learn recommend|dataset|evaluate|promote|show|revoke"))
		}
	})
}
func readBounded(r io.Reader, n int64) ([]byte, error) {
	b, e := io.ReadAll(io.LimitReader(r, n+1))
	if e != nil {
		return nil, e
	}
	if int64(len(b)) > n {
		return nil, errors.New("input exceeds byte bound")
	}
	return b, nil
}
