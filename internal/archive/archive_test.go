package archive

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/GrayCodeAI/rover/internal/access"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/store"
	"github.com/GrayCodeAI/rover/internal/testutil"
)

func TestOnlineBackupRestoreRevokesExecution(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"a.txt": "retained source\n"})
	snap, e := source.Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	task := model.TaskRun{Schema: model.Schema, ID: model.ID("task"), Status: "RUNNING", PID: os.Getpid(), Socket: "/old/worker.sock", CreatedAt: model.Now()}
	if e = s.Put("task", task.ID, task, "test.running"); e != nil {
		t.Fatal(e)
	}
	if e = s.Acquire(task.ID, []string{"db.test"}, 4); e != nil {
		t.Fatal(e)
	}
	_, token, e := access.Issue(s, repo, []string{"rover_status"}, "fixture", time.Hour)
	if e != nil {
		t.Fatal(e)
	}
	// Separate connections exercise SQLite consistency, not a raw WAL copy.
	second, e := store.Open(s.Root)
	if e != nil {
		t.Fatal(e)
	}
	defer second.Close()
	if e = second.Put("fixture", "visible", map[string]string{"data": "second connection"}, "test"); e != nil {
		t.Fatal(e)
	}
	backup := filepath.Join(t.TempDir(), "backup")
	m, e := Backup(context.Background(), s, backup)
	if e != nil {
		t.Fatal(e)
	}
	if len(m.Files) < 2 {
		t.Fatal("missing metadata/objects")
	}
	if _, e = Verify(backup); e != nil {
		t.Fatal(e)
	}
	restored := filepath.Join(t.TempDir(), "state")
	if _, e = Restore(context.Background(), backup, restored); e != nil {
		t.Fatal(e)
	}
	r, e := store.Open(restored)
	if e != nil {
		t.Fatal(e)
	}
	defer r.Close()
	var rt model.TaskRun
	if e = r.Get("task", task.ID, &rt); e != nil {
		t.Fatal(e)
	}
	if rt.Status != "LOST" || rt.PID != 0 || rt.Socket != "" {
		t.Fatalf("unsafe restored execution: %+v", rt)
	}
	if _, e = access.Authenticate(r, repo, token); e == nil {
		t.Fatal("restored bearer token still active")
	}
	leases, e := r.ListAll("lease", 100)
	if e != nil || len(leases) != 0 {
		t.Fatalf("leases retained: %s %v", leases, e)
	}
	rs, e := source.Load(r, snap.ID)
	if e != nil {
		t.Fatal(e)
	}
	if rs.ID != snap.ID {
		t.Fatal("snapshot changed")
	}
	content, e := source.FileContent(r, rs, "a.txt")
	if e != nil || string(content) != "retained source\n" {
		t.Fatal(string(content), e)
	}
	if e = r.Integrity(); e != nil {
		t.Fatal(e)
	}
	if _, e = Restore(context.Background(), backup, restored); e == nil {
		t.Fatal("overwrote existing restore")
	}
	// Backup content corruption cannot be accepted or restored.
	for _, f := range m.Files {
		if f.Path != "rover.db" {
			if e = os.WriteFile(filepath.Join(backup, f.Path), []byte("tamper"), 0600); e != nil {
				t.Fatal(e)
			}
			break
		}
	}
	if _, e = Verify(backup); e == nil {
		t.Fatal("accepted tampered backup")
	}
}
func TestBackupRejectsTraversalAndSymlinks(t *testing.T) {
	d := t.TempDir()
	m := Manifest{Schema: "rover.backup/v1", Files: []File{{Path: "../outside", Size: 0, SHA256: model.Digest(nil)}}}
	b, _ := json.Marshal(m)
	os.WriteFile(filepath.Join(d, "manifest.json"), b, 0600)
	if _, e := Verify(d); e == nil {
		t.Fatal("accepted traversal")
	}
	os.Remove(filepath.Join(d, "manifest.json"))
	target := filepath.Join(t.TempDir(), "manifest")
	os.WriteFile(target, b, 0600)
	if e := os.Symlink(target, filepath.Join(d, "manifest.json")); e != nil {
		t.Skip(e)
	}
	if _, e := Verify(d); e == nil {
		t.Fatal("accepted symlink")
	}
}
func TestBackupRejectsSymlinkedParent(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"a": "b"})
	root := t.TempDir()
	target := filepath.Join(root, "actual")
	if e := os.Mkdir(target, 0700); e != nil {
		t.Fatal(e)
	}
	link := filepath.Join(root, "link")
	if e := os.Symlink(target, link); e != nil {
		t.Skip(e)
	}
	if _, e := Backup(context.Background(), s, filepath.Join(link, "backup")); e == nil {
		t.Fatal("symlinked backup parent accepted")
	}
}
func TestCancelledBackup(t *testing.T) {
	_, s := testutil.Repo(t, map[string]string{"a": "b"})
	ctx, c := context.WithCancel(context.Background())
	c()
	dest := filepath.Join(t.TempDir(), "cancelled")
	if _, e := Backup(ctx, s, dest); e == nil {
		t.Fatal("backup ignored cancellation")
	}
	if _, e := Verify(dest); e == nil {
		t.Fatal("incomplete backup accepted")
	}
}
