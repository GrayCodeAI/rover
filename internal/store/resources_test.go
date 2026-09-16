package store

import (
	"errors"
	"path/filepath"
	"testing"
)

func TestResourceTransactions(t *testing.T) {
	s, e := Open(filepath.Join(t.TempDir(), "state"))
	if e != nil {
		t.Fatal(e)
	}
	defer s.Close()
	if e = s.Acquire("a", []string{"database"}, 2); e != nil {
		t.Fatal(e)
	}
	if e = s.Acquire("b", []string{"database"}, 2); !errors.Is(e, ErrBusy) {
		t.Fatal(e)
	}
	if e = s.Acquire("b", []string{"other"}, 2); e != nil {
		t.Fatal(e)
	}
	if e = s.Acquire("c", nil, 2); !errors.Is(e, ErrBusy) {
		t.Fatal(e)
	}
	if e = s.Release("a"); e != nil {
		t.Fatal(e)
	}
	if e = s.Acquire("c", []string{"database"}, 2); e != nil {
		t.Fatal(e)
	}
}
