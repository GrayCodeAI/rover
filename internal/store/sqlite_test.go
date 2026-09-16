package store

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sync"
	"testing"
)

func TestPersistence(t *testing.T) {
	p := filepath.Join(t.TempDir(), "state")
	s, e := Open(p)
	if e != nil {
		t.Fatal(e)
	}
	if e = s.Put("test", "one", map[string]string{"value": "persisted"}, "created"); e != nil {
		t.Fatal(e)
	}
	s.Close()
	s, e = Open(p)
	if e != nil {
		t.Fatal(e)
	}
	defer s.Close()
	var v map[string]string
	if e = s.Get("test", "one", &v); e != nil || v["value"] != "persisted" {
		t.Fatal(e, v)
	}
	if e = s.Integrity(); e != nil {
		t.Fatal(e)
	}
	ev, e := s.Events("one")
	if e != nil || len(ev) != 1 {
		t.Fatal(e, ev)
	}
}
func TestParameterizedDataAndNotFound(t *testing.T) {
	s, e := Open(t.TempDir())
	if e != nil {
		t.Fatal(e)
	}
	defer s.Close()
	text := `x'); DROP TABLE records; --`
	if e = s.Put("test", "one", text, ""); e != nil {
		t.Fatal(e)
	}
	var out string
	if e = s.Get("test", "one", &out); e != nil || out != text {
		t.Fatal(e, out)
	}
	if e = s.Get("test", text, &out); !errors.Is(e, ErrNotFound) {
		t.Fatal(e)
	}
	if s.Put("test", "../bad", nil, "") == nil {
		t.Fatal("unsafe ID")
	}
}
func TestIdempotencyAcrossConnections(t *testing.T) {
	root := t.TempDir()
	a, e := Open(root)
	if e != nil {
		t.Fatal(e)
	}
	defer a.Close()
	b, e := Open(root)
	if e != nil {
		t.Fatal(e)
	}
	defer b.Close()
	var wg sync.WaitGroup
	ch := make(chan string, 2)
	errs := make(chan error, 2)
	for i, s := range []*Store{a, b} {
		wg.Add(1)
		go func(i int, s *Store) {
			defer wg.Done()
			id := "one"
			if i == 1 {
				id = "two"
			}
			actual, _, e := s.CreateOnce("test", id, "same-key", "hash", map[string]string{"id": id})
			if e != nil {
				errs <- e
			} else {
				ch <- actual
			}
		}(i, s)
	}
	wg.Wait()
	close(ch)
	close(errs)
	for e := range errs {
		t.Fatal(e)
	}
	last := ""
	for id := range ch {
		if last != "" && id != last {
			t.Fatal("duplicate requests")
		}
		last = id
	}
	if _, _, e = a.CreateOnce("test", "other", "same-key", "different", nil); e == nil {
		t.Fatal("changed payload accepted")
	}
	items, e := a.List("test", 10)
	if e != nil || len(items) != 1 {
		t.Fatal(e, len(items))
	}
}
func TestMutationAtomicity(t *testing.T) {
	s, e := Open(t.TempDir())
	if e != nil {
		t.Fatal(e)
	}
	defer s.Close()
	s.Put("n", "one", 0, "")
	var wg sync.WaitGroup
	for i := 0; i < 20; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			e := s.Mutate("n", "one", "increment", func(b json.RawMessage) (any, error) { var n int; json.Unmarshal(b, &n); return n + 1, nil })
			if e != nil {
				t.Error(e)
			}
		}()
	}
	wg.Wait()
	var n int
	s.Get("n", "one", &n)
	if n != 20 {
		t.Fatal(n)
	}
}
func TestBlobAtomicAndTamper(t *testing.T) {
	s, e := Open(t.TempDir())
	if e != nil {
		t.Fatal(e)
	}
	defer s.Close()
	data := []byte("same object")
	var wg sync.WaitGroup
	for i := 0; i < 20; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			if _, e := s.Blob(data); e != nil {
				t.Error(e)
			}
		}()
	}
	wg.Wait()
	h, e := s.Blob(data)
	if e != nil {
		t.Fatal(e)
	}
	os.WriteFile(filepath.Join(s.Root, "objects", h), []byte("altered"), 0600)
	if _, e = s.ReadBlob(h); e == nil {
		t.Fatal("tampered object accepted")
	}
	if _, e = s.ReadBlob("../../secret"); e == nil {
		t.Fatal("unsafe digest accepted")
	}
}
func TestSymlinkStateRejected(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "actual")
	os.Mkdir(target, 0700)
	link := filepath.Join(root, "link")
	if e := os.Symlink(target, link); e != nil {
		t.Skip(e)
	}
	if s, e := Open(link); e == nil {
		s.Close()
		t.Fatal("symlink accepted")
	}
}
func TestIntermediateSymlinkAncestorRejected(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "actual")
	if e := os.Mkdir(target, 0700); e != nil {
		t.Fatal(e)
	}
	link := filepath.Join(root, "link")
	if e := os.Symlink(target, link); e != nil {
		t.Skip(e)
	}
	if s, e := Open(filepath.Join(link, "sub")); e == nil {
		s.Close()
		t.Fatal("intermediate symlink accepted")
	}
}
func TestUnresolvedTempStateOpensAtResolvedRoot(t *testing.T) {
	raw := filepath.Join(t.TempDir(), "state")
	s, e := Open(raw)
	if e != nil {
		t.Fatal(e)
	}
	defer s.Close()
	resolved, e := filepath.EvalSymlinks(raw)
	if e != nil {
		t.Fatal(e)
	}
	if s.Root != resolved {
		t.Fatalf("root %q != resolved %q", s.Root, resolved)
	}
}
func TestNewerSchemaRejected(t *testing.T) {
	root := t.TempDir()
	s, e := Open(root)
	if e != nil {
		t.Fatal(e)
	}
	s.query("PRAGMA user_version=99")
	s.Close()
	if newer, e := Open(root); e == nil {
		newer.Close()
		t.Fatal("future schema accepted")
	}
}

func TestRejectUnrelatedStateBeforeChangingPermissions(t *testing.T) {
	dir := t.TempDir()
	if e := os.Chmod(dir, 0755); e != nil {
		t.Fatal(e)
	}
	if e := os.WriteFile(filepath.Join(dir, "unrelated.txt"), []byte("keep"), 0600); e != nil {
		t.Fatal(e)
	}
	if db, e := Open(dir); e == nil {
		db.Close()
		t.Fatal("accepted unrelated directory")
	}
	info, e := os.Stat(dir)
	if e != nil {
		t.Fatal(e)
	}
	if info.Mode().Perm() != 0755 {
		t.Fatal("changed unrelated directory permissions")
	}
	if _, e := os.Stat(filepath.Join(dir, "objects")); !os.IsNotExist(e) {
		t.Fatal("created state in unrelated directory")
	}
	for _, p := range []string{"/", "."} {
		if db, e := Open(p); e == nil {
			db.Close()
			t.Fatalf("accepted %q", p)
		}
	}
}
