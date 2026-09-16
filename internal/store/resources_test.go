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

// TestPortReservation covers A12: a named TCP port is a single-slot resource.
// A second owner is refused while held and can acquire it after Release. This
// is local coordination, not OS-level port binding or container isolation.
func TestPortReservation(t *testing.T) {
	s, e := Open(filepath.Join(t.TempDir(), "state"))
	if e != nil {
		t.Fatal(e)
	}
	defer s.Close()
	if e = s.Acquire("svc_a", []string{"port-8080"}, 4); e != nil {
		t.Fatal(e)
	}
	if e = s.Acquire("svc_b", []string{"port-8080"}, 4); !errors.Is(e, ErrBusy) {
		t.Fatalf("want ErrBusy for held port, got %v", e)
	}
	if e = s.Acquire("svc_b", []string{"port-9090"}, 4); e != nil {
		t.Fatal(e)
	}
	if e = s.Release("svc_a"); e != nil {
		t.Fatal(e)
	}
	if e = s.Acquire("svc_c", []string{"port-8080"}, 4); e != nil {
		t.Fatalf("port not released: %v", e)
	}
}

// TestDatabaseReservation covers A12 for named databases: acquisition is
// exclusive per name, and distinct names coexist.
func TestDatabaseReservation(t *testing.T) {
	s, e := Open(filepath.Join(t.TempDir(), "state"))
	if e != nil {
		t.Fatal(e)
	}
	defer s.Close()
	if e = s.Acquire("job1", []string{"db-analytics"}, 4); e != nil {
		t.Fatal(e)
	}
	if e = s.Acquire("job2", []string{"db-analytics"}, 4); !errors.Is(e, ErrBusy) {
		t.Fatalf("want ErrBusy for held db, got %v", e)
	}
	if e = s.Acquire("job2", []string{"db-reporting"}, 4); e != nil {
		t.Fatal(e)
	}
	if e = s.Release("job1"); e != nil {
		t.Fatal(e)
	}
	if e = s.Acquire("job3", []string{"db-analytics"}, 4); e != nil {
		t.Fatalf("db not released: %v", e)
	}
}
