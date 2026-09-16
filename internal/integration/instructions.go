// Package integration installs reversible guidance, not security enforcement.
package integration

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/store"
)

const begin = "<!-- ROVER MANAGED GUIDANCE BEGIN -->"
const end = "<!-- ROVER MANAGED GUIDANCE END -->"
const guidance = begin + `
## Rover workflow
Rover is optional tooling, not permission to execute arbitrary commands.
- Inspect the installed version and actual capabilities with ` + "`rover doctor --json`" + `.
- Review repository setup with ` + "`rover init --repo . --json`" + `; do not apply changes without authorization.
- Use the approved baseline, task scope, execution mode and checks. Do not assume a branch named main.
- Capture a candidate and run configured Rover verification only with the user's execution permission.
- Read structured check outcomes and the acceptance decision separately from process exit/submission success.
- Repair defects without silently weakening tests, policy, task scope or protected acceptance inputs.
- A changed candidate invalidates prior evidence. Preserve failed attempts and report unknowns.
- Do not claim review, safety, proof, merge or delivery unless that exact action/result is recorded.
- Instructions are guidance; independently controlled policy and CI provide mandatory enforcement.
` + end

type Plan struct {
	Schema     string `json:"schema"`
	Repository string `json:"repository"`
	Path       string `json:"path"`
	Agent      string `json:"agent"`
	Before     string `json:"before_sha256"`
	After      string `json:"after_sha256"`
	Content    string `json:"content"`
	Exists     bool   `json:"exists"`
	Changed    bool   `json:"changed"`
}
type Record struct {
	Schema     string `json:"schema"`
	ID         string `json:"id"`
	Plan       Plan   `json:"plan"`
	BeforeBlob string `json:"before_blob"`
	Mode       uint32 `json:"mode"`
	Status     string `json:"status"`
	At         string `json:"at"`
}

func target(agent string) (string, error) {
	switch agent {
	case "generic", "codex", "opencode":
		return "AGENTS.md", nil
	case "claude":
		return "CLAUDE.md", nil
	case "gemini":
		return "GEMINI.md", nil
	}
	return "", errors.New("supported guidance targets: generic, codex, opencode, claude, gemini")
}
func read(path string) ([]byte, os.FileMode, bool, error) {
	st, e := os.Lstat(path)
	if errors.Is(e, os.ErrNotExist) {
		return []byte{}, 0644, false, nil
	}
	if e != nil {
		return nil, 0, false, e
	}
	if !st.Mode().IsRegular() || st.Size() > 1<<20 {
		return nil, 0, false, errors.New("instruction file must be a regular file no larger than 1 MiB")
	}
	b, e := os.ReadFile(path)
	if len(b) > 1<<20 {
		return nil, 0, false, errors.New("instruction file grew")
	}
	return b, st.Mode().Perm(), true, e
}
func Preview(ctx context.Context, repo, agent string) (Plan, error) {
	p := Plan{Schema: model.Schema, Agent: agent}
	root, e := source.Discover(ctx, repo)
	if e != nil {
		return p, e
	}
	name, e := target(agent)
	if e != nil {
		return p, e
	}
	p.Repository = root
	p.Path = name
	b, _, exists, e := read(filepath.Join(root, name))
	if e != nil {
		return p, e
	}
	p.Exists = exists
	p.Before = model.Digest(b)
	text := string(b)
	a, z := strings.Count(text, begin), strings.Count(text, end)
	if a != z || a > 1 {
		return p, errors.New("ambiguous Rover guidance markers; no edit performed")
	}
	if a == 1 {
		start, stop := strings.Index(text, begin), strings.Index(text, end)
		if stop < start {
			return p, errors.New("reversed markers")
		}
		p.Content = text[:start] + guidance + text[stop+len(end):]
	} else {
		p.Content = text
		if text != "" && !strings.HasSuffix(text, "\n") {
			p.Content += "\n"
		}
		if text != "" {
			p.Content += "\n"
		}
		p.Content += guidance + "\n"
	}
	p.After = model.Digest([]byte(p.Content))
	p.Changed = p.Before != p.After
	return p, nil
}
func replace(path string, b []byte, mode os.FileMode) error {
	f, e := os.CreateTemp(filepath.Dir(path), ".rover-guide-")
	if e != nil {
		return e
	}
	defer os.Remove(f.Name())
	if e = f.Chmod(mode); e != nil {
		f.Close()
		return e
	}
	if _, e = f.Write(b); e != nil {
		f.Close()
		return e
	}
	if e = f.Sync(); e != nil {
		f.Close()
		return e
	}
	if e = f.Close(); e != nil {
		return e
	}
	if e = os.Rename(f.Name(), path); e != nil {
		return e
	}
	return store.SyncDir(filepath.Dir(path))
}
func Apply(ctx context.Context, s *store.Store, p Plan) (Record, error) {
	r := Record{Schema: model.Schema, ID: model.ID("guide"), Plan: p, At: model.Now()}
	fresh, e := Preview(ctx, p.Repository, p.Agent)
	if e != nil {
		return r, e
	}
	if model.Hash(fresh) != model.Hash(p) {
		return r, errors.New("setup preview is stale; no edit performed")
	}
	if !p.Changed {
		return r, errors.New("guidance already matches; no edit required")
	}
	b, mode, _, e := read(filepath.Join(p.Repository, p.Path))
	if e != nil {
		return r, e
	}
	r.Mode = uint32(mode)
	r.BeforeBlob, e = s.Blob(b)
	if e != nil {
		return r, e
	}
	r.Status = "PREPARED"
	if e = s.Put("integration", r.ID, r, "integration.prepared"); e != nil {
		return r, e
	}
	// Preserve and journal before the edit. A crash leaves a PREPARED record;
	// inspect actual file hashes instead of blindly repeating or overwriting.
	if e = replace(filepath.Join(p.Repository, p.Path), []byte(p.Content), mode); e != nil {
		return r, e
	}
	r.Status = "APPLIED"
	return r, s.Put("integration", r.ID, r, "integration.applied")
}
func Undo(s *store.Store, id string) (Record, error) {
	var r Record
	if e := s.Get("integration", id, &r); e != nil {
		return r, e
	}
	if r.Status != "APPLIED" && r.Status != "PREPARED" {
		return r, errors.New("integration not active")
	}
	name, e := target(r.Plan.Agent)
	if e != nil || name != r.Plan.Path {
		return r, errors.New("invalid integration path")
	}
	path := filepath.Join(r.Plan.Repository, name)
	b, _, _, e := read(path)
	if e != nil {
		return r, e
	}
	if model.Digest(b) != r.Plan.After {
		return r, errors.New("instruction file changed since integration; undo refused")
	}
	old, e := s.ReadBlob(r.BeforeBlob)
	if e != nil {
		return r, e
	}
	if model.Digest(old) != r.Plan.Before {
		return r, fmt.Errorf("backup identity mismatch")
	}
	if r.Plan.Exists {
		e = replace(path, old, os.FileMode(r.Mode)&0777)
	} else {
		e = os.Remove(path)
	}
	if e != nil {
		return r, e
	}
	r.Status = "UNDONE"
	return r, s.Put("integration", r.ID, r, "integration.undone")
}
