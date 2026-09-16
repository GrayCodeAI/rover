package integration

import (
	"context"
	"github.com/GrayCodeAI/rover/internal/testutil"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestPreviewApplyUndoPreservesUserInstructions(t *testing.T) {
	original := "# Existing instructions\nNever overwrite these.\n"
	repo, s := testutil.Repo(t, map[string]string{"AGENTS.md": original})
	p, e := Preview(context.Background(), repo, "codex")
	if e != nil {
		t.Fatal(e)
	}
	b, _ := os.ReadFile(filepath.Join(repo, "AGENTS.md"))
	if string(b) != original {
		t.Fatal("preview mutated file")
	}
	if !strings.HasPrefix(p.Content, original) {
		t.Fatal("original lost")
	}
	r, e := Apply(context.Background(), s, p)
	if e != nil {
		t.Fatal(e)
	}
	again, e := Preview(context.Background(), repo, "generic")
	if e != nil || again.Changed {
		t.Fatal(again, e)
	}
	b, _ = os.ReadFile(filepath.Join(repo, "AGENTS.md"))
	if strings.Count(string(b), begin) != 1 {
		t.Fatal("duplicate markers")
	}
	os.WriteFile(filepath.Join(repo, "AGENTS.md"), append(b, []byte("human edit\n")...), 0644)
	if _, e = Undo(s, r.ID); e == nil {
		t.Fatal("clobbered newer human edit")
	}
	os.WriteFile(filepath.Join(repo, "AGENTS.md"), b, 0644)
	if _, e = Undo(s, r.ID); e != nil {
		t.Fatal(e)
	}
	b, _ = os.ReadFile(filepath.Join(repo, "AGENTS.md"))
	if string(b) != original {
		t.Fatal("undo didn't restore original")
	}
}
func TestStalePreviewAndSymlinkRefused(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"AGENTS.md": "old"})
	p, e := Preview(context.Background(), repo, "codex")
	if e != nil {
		t.Fatal(e)
	}
	os.WriteFile(filepath.Join(repo, "AGENTS.md"), []byte("new"), 0644)
	if _, e = Apply(context.Background(), s, p); e == nil {
		t.Fatal("stale preview accepted")
	}
	os.Remove(filepath.Join(repo, "AGENTS.md"))
	outside := filepath.Join(t.TempDir(), "secret")
	os.WriteFile(outside, []byte("secret"), 0600)
	if e = os.Symlink(outside, filepath.Join(repo, "AGENTS.md")); e != nil {
		t.Skip(e)
	}
	if _, e = Preview(context.Background(), repo, "codex"); e == nil {
		t.Fatal("followed symlink")
	}
}
func TestNewFileUndo(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"README": "repo"})
	p, e := Preview(context.Background(), repo, "claude")
	if e != nil {
		t.Fatal(e)
	}
	r, e := Apply(context.Background(), s, p)
	if e != nil {
		t.Fatal(e)
	}
	if _, e = Undo(s, r.ID); e != nil {
		t.Fatal(e)
	}
	if _, e = os.Stat(filepath.Join(repo, "CLAUDE.md")); !os.IsNotExist(e) {
		t.Fatal("new instruction file not removed")
	}
}
