package source

import (
	"context"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/testutil"
	"testing"
)

func TestOverlayAndMerge(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"a": "old", "b": "old"})
	base, e := Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	makeSnap := func(p, v string) model.Snapshot {
		fs := append([]model.File(nil), base.Files...)
		h, e := s.Blob([]byte(v))
		if e != nil {
			t.Fatal(e)
		}
		for i := range fs {
			if fs[i].Path == p {
				fs[i].SHA256 = h
				fs[i].Size = int64(len(v))
			}
		}
		n, e := Compose(s, base, fs, "test")
		if e != nil {
			t.Fatal(e)
		}
		return n
	}
	a := makeSnap("a", "new-a")
	b := makeSnap("b", "new-b")
	merged, e := Merge(s, base, []model.Snapshot{a, b})
	if e != nil {
		t.Fatal(e)
	}
	v, _ := FileContent(s, merged, "a")
	if string(v) != "new-a" {
		t.Fatal(string(v))
	}
	if _, e = Merge(s, base, []model.Snapshot{a, makeSnap("a", "conflict")}); e == nil {
		t.Fatal("conflict ignored")
	}
	over, e := Overlay(s, base, a, []string{"a"})
	if e != nil || over.ID != a.ID {
		t.Fatal(e)
	}
}
