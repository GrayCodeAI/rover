package source

import (
	"context"
	"github.com/GrayCodeAI/rover/internal/testutil"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestCommitSnapshotFrozen(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "old"})
	snap, e := Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	testutil.Write(t, repo, "value", "new")
	dir := filepath.Join(t.TempDir(), "checkout")
	if e = Materialize(s, snap, dir); e != nil {
		t.Fatal(e)
	}
	b, e := os.ReadFile(filepath.Join(dir, "value"))
	if e != nil || string(b) != "old" {
		t.Fatal(e, string(b))
	}
	if e = InputsUnchanged(snap, dir); e != nil {
		t.Fatal(e)
	}
	testutil.Write(t, dir, "value", "modified")
	if InputsUnchanged(snap, dir) == nil {
		t.Fatal("modified input accepted")
	}
}
func TestWorkingTreeRequiresUntrackedOptIn(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	testutil.Write(t, repo, "new.txt", "untracked")
	a, e := Capture(context.Background(), s, repo, "WORKTREE", false)
	if e != nil {
		t.Fatal(e)
	}
	b, e := Capture(context.Background(), s, repo, "WORKTREE", true)
	if e != nil {
		t.Fatal(e)
	}
	if len(a.Files) != 1 || len(b.Files) != 2 || a.ID == b.ID {
		t.Fatal(a, b)
	}
}
func TestSourceStatesRejected(t *testing.T) {
	for _, name := range []string{"symlink", "LFS", "case-collision"} {
		t.Run(name, func(t *testing.T) {
			repo, s := testutil.Repo(t, map[string]string{"value": "base"})
			switch name {
			case "symlink":
				os.Symlink("/etc/passwd", filepath.Join(repo, "link"))
			case "LFS":
				testutil.Write(t, repo, "pointer", "version https://git-lfs.github.com/spec/v1\noid sha256:000\n")
			case "case-collision":
				// Case-collision rejection is only testable on a case-sensitive
				// filesystem. On case-insensitive volumes (default macOS APFS)
				// both names address one file, so no collision exists to reject.
				probe := filepath.Join(repo, ".rover-case-probe-TMP")
				testutil.Write(t, repo, ".rover-case-probe-TMP", "a")
				if _, e := os.Stat(filepath.Join(repo, ".rover-case-probe-tmp")); e == nil {
					os.Remove(probe)
					t.Skip("case-insensitive filesystem: collision scenario not representable")
				}
				os.Remove(probe)
				testutil.Write(t, repo, "VALUE", "another")
			}
			if _, e := Capture(context.Background(), s, repo, "WORKTREE", true); e == nil {
				t.Fatal("unsupported source accepted")
			}
		})
	}
}
func TestGitCallbacksNotExecuted(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	marker := filepath.Join(t.TempDir(), "executed")
	testutil.Git(t, repo, "config", "core.fsmonitor", "touch "+marker)
	testutil.Git(t, repo, "config", "diff.external", "touch "+marker)
	if _, e := Capture(context.Background(), s, repo, "HEAD", false); e != nil {
		t.Fatal(e)
	}
	if _, e := os.Stat(marker); !os.IsNotExist(e) {
		t.Fatal("repository callback executed")
	}
}
func TestWorktreeNoSmudgeExecution(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base", ".gitattributes": "value filter=evil\n"})
	marker := filepath.Join(t.TempDir(), "executed")
	testutil.Git(t, repo, "config", "filter.evil.smudge", "touch "+marker)
	snap, e := Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	dir := filepath.Join(s.Root, "worktree")
	if e = AddWorktree(context.Background(), s, snap, dir); e != nil {
		t.Fatal(e)
	}
	if _, e := os.Stat(marker); !os.IsNotExist(e) {
		t.Fatal("smudge executed")
	}
	if st, e := os.Stat(filepath.Join(dir, ".git")); e != nil || !st.Mode().IsRegular() {
		t.Fatal("worktree not created", e)
	}
}
func TestDiffCategories(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"tests/a_test.go": "test", "value": "base"})
	a, e := Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	os.Remove(filepath.Join(repo, "tests/a_test.go"))
	testutil.Write(t, repo, ".github/workflows/ci.yml", "changed")
	testutil.Write(t, repo, ".rover/config.json", "{}")
	b, e := Capture(context.Background(), s, repo, "WORKTREE", true)
	if e != nil {
		t.Fatal(e)
	}
	d := Compare(a, b)
	if len(d.Findings) != 3 {
		t.Fatal(d.Findings)
	}
}
func TestTamperedManifestRejected(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	snap, e := Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	snap.Files[0].Path = "../../escape"
	if Materialize(s, snap, t.TempDir()) == nil {
		t.Fatal("invalid manifest accepted")
	}
}
func TestSafeName(t *testing.T) {
	for _, p := range []string{"../bad", "/abs", "a/../b", "a\\b", ".git/config", "x/.GIT/z", ""} {
		if safeName(p) == nil {
			t.Errorf("accepted %q", p)
		}
	}
}
func TestResolveOptionRejected(t *testing.T) {
	repo, _ := testutil.Repo(t, nil)
	if _, e := Resolve(context.Background(), repo, "--help"); e == nil {
		t.Fatal("option accepted")
	}
}

// TestCaptureConsistencyLabels covers A10/A11: the snapshot must say exactly
// what it is — double identical reads frozen, not a filesystem-atomic capture.
func TestCaptureConsistencyLabels(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	head, e := Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil || !strings.Contains(head.Consistency, "exact Git commit blobs") {
		t.Fatalf("HEAD label: %q %v", head.Consistency, e)
	}
	wt, e := Capture(context.Background(), s, repo, "WORKTREE", true)
	if e != nil || !strings.Contains(wt.Consistency, "not a filesystem-atomic capture") {
		t.Fatalf("WORKTREE label: %q %v", wt.Consistency, e)
	}
}

// TestMutationDetectedAfterCapture covers the same documented boundary from the
// verification side: bytes captured are frozen, and a post-capture mutation is
// caught by InputsUnchanged rather than silently accepted.
func TestMutationDetectedAfterCapture(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"value": "base"})
	snap, e := Capture(context.Background(), s, repo, "WORKTREE", false)
	if e != nil {
		t.Fatal(e)
	}
	if e = InputsUnchanged(snap, repo); e != nil {
		t.Fatal(e)
	}
	testutil.Write(t, repo, "value", "mutated-during-capture")
	if e = InputsUnchanged(snap, repo); e == nil {
		t.Fatal("post-capture mutation not detected")
	}
	if b, e := FileContent(s, snap, "value"); e != nil || string(b) != "base" {
		t.Fatalf("frozen byte lost: %q %v", b, e)
	}
}
func TestNestedSymlinkReadRejected(t *testing.T) {
	root := t.TempDir()
	outside := t.TempDir()
	os.WriteFile(filepath.Join(outside, "data"), []byte("secret"), 0600)
	os.Symlink(outside, filepath.Join(root, "dir"))
	if _, _, e := ReadRegular(root, "dir/data", 100); e == nil {
		t.Fatal("symlink followed")
	}
}
func FuzzSafeName(f *testing.F) {
	for _, p := range []string{"a.go", "../x", "a/.git/x", "foo\nbar", "--foo"} {
		f.Add(p)
	}
	f.Fuzz(func(t *testing.T, p string) {
		if safeName(p) == nil && (strings.Contains(p, "\\") || filepath.IsAbs(p)) {
			t.Fatal("unsafe accepted")
		}
	})
}
