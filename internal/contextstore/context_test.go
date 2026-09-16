package contextstore

import (
	"context"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/testutil"
	"testing"
	"time"
)

func TestScopedContextAndMemory(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"a.go": "package app\n// lookup token\n", ".env": "token=secret"})
	snap, e := source.Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	r, e := Search(context.Background(), s, snap, "token", 20)
	if e != nil || len(r.Matches) != 1 || r.Excluded != 1 {
		t.Fatal(r, e)
	}
	if _, e = Build(s, snap, []string{".env"}); e == nil {
		t.Fatal("sensitive file included")
	}
	b, e := Build(s, snap, []string{"a.go"})
	if e != nil || b.Snapshot != snap.ID {
		t.Fatal(b, e)
	}
	n, e := PutNote(s, snap, "approved historical note", "human", time.Hour)
	if e != nil {
		t.Fatal(e)
	}
	rows, e := Notes(s, snap)
	if e != nil || len(rows) != 1 || rows[0].Stale {
		t.Fatal(rows, e)
	}
	other := snap
	other.Repository = "/other-project"
	rows, e = Notes(s, other)
	if e != nil || len(rows) != 0 {
		t.Fatal("cross-project note", rows, e)
	}
	n.ExpiresAt = time.Now().Add(-time.Minute).Format(time.RFC3339Nano)
	_ = s.Put("memory", n.ID, n, "")
	rows, e = Notes(s, snap)
	if e != nil || len(rows) != 0 {
		t.Fatal("expired note", rows, e)
	}
}
