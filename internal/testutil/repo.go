// Package testutil contains isolated fixtures. It is only used by tests.
package testutil

import (
	"github.com/GrayCodeAI/rover/internal/store"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func Git(t *testing.T, repo string, args ...string) string {
	t.Helper()
	fixed := []string{"-c", "core.hooksPath=" + os.DevNull, "-c", "commit.gpgsign=false", "-c", "core.fsmonitor=false", "-C", repo}
	cmd := exec.Command("git", append(fixed, args...)...)
	cmd.Env = append(os.Environ(), "GIT_CONFIG_GLOBAL="+os.DevNull, "GIT_CONFIG_NOSYSTEM=1", "GIT_AUTHOR_NAME=Rover Test", "GIT_AUTHOR_EMAIL=test@example.invalid", "GIT_COMMITTER_NAME=Rover Test", "GIT_COMMITTER_EMAIL=test@example.invalid")
	b, e := cmd.CombinedOutput()
	if e != nil {
		t.Fatalf("git %v: %v: %s", args, e, b)
	}
	return strings.TrimSpace(string(b))
}
func Write(t *testing.T, repo, p, body string) {
	t.Helper()
	target := filepath.Join(repo, p)
	if e := os.MkdirAll(filepath.Dir(target), 0700); e != nil {
		t.Fatal(e)
	}
	if e := os.WriteFile(target, []byte(body), 0600); e != nil {
		t.Fatal(e)
	}
}
func Commit(t *testing.T, repo string) string {
	t.Helper()
	Git(t, repo, "add", "-A")
	Git(t, repo, "commit", "-m", "fixture")
	return Git(t, repo, "rev-parse", "HEAD")
}
func Repo(t *testing.T, files map[string]string) (string, *store.Store) {
	t.Helper()
	tmp := t.TempDir()
	// On darwin t.TempDir() sits under /var (a symlink to /private/var).
	// Canonicalize so repository and state paths share one identity.
	if resolved, e := filepath.EvalSymlinks(tmp); e == nil {
		tmp = resolved
	}
	root := tmp
	repo := filepath.Join(root, "repo")
	if e := os.Mkdir(repo, 0700); e != nil {
		t.Fatal(e)
	}
	Git(t, repo, "init", "-b", "main")
	for p, b := range files {
		Write(t, repo, p, b)
	}
	if len(files) == 0 {
		Write(t, repo, "README.md", "fixture")
	}
	Commit(t, repo)
	s, e := store.Open(filepath.Join(root, "state"))
	if e != nil {
		t.Fatal(e)
	}
	t.Cleanup(func() { s.Close() })
	return repo, s
}
