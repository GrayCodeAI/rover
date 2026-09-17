// Package access supplies revocable, project/audience/tool-scoped bearer grants.
// It is not an organization identity provider; the local operator issues grants.
package access

import (
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"errors"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/store"
	"path/filepath"
	"strings"
	"time"
)

var ErrDenied = errors.New("unauthorized or expired grant")

// allowedTools mirrors service.AllNames without importing service (which
// imports access). Keep in sync; service_test asserts equality.
var allowedTools = map[string]bool{
	"rover_agent_capabilities": true,
	"rover_inspect":            true,
	"rover_status":             true,
	"rover_report":             true,
	"rover_diff":               true,
	"rover_context_search":     true,
	"rover_verify":             true,
	"rover_task_run":           true,
	"rover_task_cancel":        true,
}

func canonicalRepo(repo string) string {
	if repo == "" {
		return repo
	}
	if a, e := filepath.Abs(repo); e == nil {
		if r, e := filepath.EvalSymlinks(a); e == nil {
			return r
		}
		return filepath.Clean(a)
	}
	return repo
}

type Grant struct {
	Schema    string   `json:"schema"`
	ID        string   `json:"id"`
	Audience  string   `json:"audience"`
	Project   string   `json:"project"`
	Tools     []string `json:"tools"`
	Note      string   `json:"note"`
	CreatedAt string   `json:"created_at"`
	ExpiresAt string   `json:"expires_at"`
	Revoked   bool     `json:"revoked"`
}

func Audience(s *store.Store, repo string) string {
	repo = canonicalRepo(repo)
	return "rover-control/v1:" + model.Digest([]byte(s.Root+"\x00"+repo))
}
func ProjectID(repo string) string {
	return model.Digest([]byte(canonicalRepo(repo)))
}
func Issue(s *store.Store, repo string, tools []string, note string, ttl time.Duration) (Grant, string, error) {
	if len(tools) == 0 || len(tools) > 32 || ttl < time.Minute || ttl > 30*24*time.Hour || strings.TrimSpace(note) == "" || len(note) > 4096 {
		return Grant{}, "", errors.New("tools, reason and TTL 1m..30d required")
	}
	seen := map[string]bool{}
	for _, t := range tools {
		if !model.ValidID(t) || !allowedTools[t] || seen[t] {
			return Grant{}, "", errors.New("invalid duplicate tool")
		}
		seen[t] = true
	}
	repo = canonicalRepo(repo)
	b := make([]byte, 32)
	if _, e := rand.Read(b); e != nil {
		return Grant{}, "", e
	}
	token := "rvr_" + base64.RawURLEncoding.EncodeToString(b)
	g := Grant{Schema: model.Schema, ID: model.Digest([]byte(token)), Audience: Audience(s, repo), Project: ProjectID(repo), Tools: append([]string(nil), tools...), Note: note, CreatedAt: model.Now(), ExpiresAt: time.Now().UTC().Add(ttl).Format(time.RFC3339Nano)}
	return g, token, s.Put("grant", g.ID, g, "grant.created")
}
func Authenticate(s *store.Store, repo, token string) (Grant, error) {
	var g Grant
	if len(token) != 47 || !strings.HasPrefix(token, "rvr_") {
		return g, ErrDenied
	}
	if e := s.Get("grant", model.Digest([]byte(token)), &g); e != nil {
		return g, ErrDenied
	}
	repo = canonicalRepo(repo)
	exp, e := time.Parse(time.RFC3339Nano, g.ExpiresAt)
	if e != nil || g.Revoked || !time.Now().Before(exp) || g.Audience != Audience(s, repo) || g.Project != ProjectID(repo) {
		return Grant{}, ErrDenied
	}
	return g, nil
}
func (g Grant) Allows(tool string) bool {
	for _, t := range g.Tools {
		if t == tool {
			return true
		}
	}
	return false
}
func Revoke(s *store.Store, id string) error {
	return s.Mutate("grant", id, "grant.revoked", func(b json.RawMessage) (any, error) {
		if b == nil {
			return nil, store.ErrNotFound
		}
		var g Grant
		if e := json.Unmarshal(b, &g); e != nil {
			return nil, e
		}
		g.Revoked = true
		return g, nil
	})
}
