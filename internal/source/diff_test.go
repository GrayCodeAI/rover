package source

import (
	"context"
	"github.com/GrayCodeAI/rover/internal/testutil"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestApplicableSnapshotPatch(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"a.txt": "old\n", "drop.txt": "remove\n", "space name.txt": "before\n"})
	base, e := Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	testutil.Write(t, repo, "a.txt", "new\n")
	testutil.Write(t, repo, "space name.txt", "after\n")
	testutil.Write(t, repo, "binary.bin", "\x00\x01\x02")
	os.Remove(filepath.Join(repo, "drop.txt"))
	testutil.Commit(t, repo)
	cand, e := Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	b, e := Diff(context.Background(), s, base, cand)
	if e != nil {
		t.Fatal(e)
	}
	if !strings.Contains(string(b), "diff --git a/a.txt b/a.txt") {
		t.Fatal(string(b))
	}
	testutil.Git(t, repo, "checkout", "--detach", base.Commit)
	patch := filepath.Join(t.TempDir(), "change.patch")
	if e = os.WriteFile(patch, b, 0600); e != nil {
		t.Fatal(e)
	}
	testutil.Git(t, repo, "apply", "--check", patch)
	testutil.Git(t, repo, "apply", patch)
	actual, e := Capture(context.Background(), s, repo, "WORKTREE", true)
	if e != nil || actual.ID != cand.ID {
		t.Fatalf("patch identity mismatch %s %s %v", actual.ID, cand.ID, e)
	}
}
