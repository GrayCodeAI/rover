package access

import (
	"errors"
	"github.com/GrayCodeAI/rover/internal/store"
	"path/filepath"
	"testing"
	"time"
)

func TestGrantScopesAndRevocation(t *testing.T) {
	s, e := store.Open(filepath.Join(t.TempDir(), "state"))
	if e != nil {
		t.Fatal(e)
	}
	defer s.Close()
	g, token, e := Issue(s, "/project/a", []string{"rover_status"}, "owned client", time.Hour)
	if e != nil {
		t.Fatal(e)
	}
	got, e := Authenticate(s, "/project/a", token)
	if e != nil || got.ID != g.ID || !got.Allows("rover_status") || got.Allows("rover_verify") {
		t.Fatal(got, e)
	}
	if _, e = Authenticate(s, "/project/b", token); !errors.Is(e, ErrDenied) {
		t.Fatal(e)
	}
	if e = Revoke(s, g.ID); e != nil {
		t.Fatal(e)
	}
	if _, e = Authenticate(s, "/project/a", token); !errors.Is(e, ErrDenied) {
		t.Fatal(e)
	}
	g, token, e = Issue(s, "/project/a", []string{"rover_status"}, "expiry", time.Hour)
	if e != nil {
		t.Fatal(e)
	}
	g.ExpiresAt = time.Now().Add(-time.Second).Format(time.RFC3339Nano)
	_ = s.Put("grant", g.ID, g, "")
	if _, e = Authenticate(s, "/project/a", token); !errors.Is(e, ErrDenied) {
		t.Fatal(e)
	}
}

func TestGrantToolAllowlist(t *testing.T) {
	s, e := store.Open(filepath.Join(t.TempDir(), "state"))
	if e != nil {
		t.Fatal(e)
	}
	defer s.Close()
	if _, _, e := Issue(s, "/p", []string{"rover_status_typo"}, "x", time.Hour); e == nil {
		t.Fatal("undeclared tool accepted")
	}
	if _, _, e := Issue(s, "/p", []string{"rover_status", "rover_status"}, "x", time.Hour); e == nil {
		t.Fatal("duplicate tool accepted")
	}
}

func TestGrantRepoCanonicalization(t *testing.T) {
	// Trailing-slash and dot variants must scope identically.
	if ProjectID("/project/a") != ProjectID("/project/a/") {
		t.Fatal("trailing slash changes project scope")
	}
	if ProjectID("/project/a") != ProjectID("/project/a/.") {
		t.Fatal("dot path changes project scope")
	}
}
