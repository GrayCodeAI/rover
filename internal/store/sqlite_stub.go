//go:build !cgo

// Package store stub for CGO_ENABLED=0 builds (e.g. cross-compilation targets
// where the system SQLite C library is unavailable). The real implementation
// lives in sqlite.go and requires cgo + libsqlite3. This stub allows the
// package to compile and link but returns errNoCGO for all SQLite-dependent
// operations. Pure-Go helpers (PrivateDir, SyncDir, AtomicFile, BoundedFile,
// IsWithin, DefaultRoot) are fully functional.
package store

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"syscall"

	"github.com/GrayCodeAI/rover/internal/model"
)

// ErrNotFound is defined here for the stub; budget.go and resources.go
// reference it but do not redefine it (they are always compiled).
var ErrNotFound = errors.New("record not found")

var errNoCGO = errors.New("store requires CGO and system SQLite")

var digestRE = regexp.MustCompile(`^[0-9a-f]{64}$`)

// Store is a stub type. When cgo is enabled, sqlite.go provides the real
// implementation backed by SQLite. Without cgo, all database operations return
// errNoCGO.
type Store struct {
	mu   sync.Mutex
	Root string
	// DBPath is the absolute path to the SQLite database file.
	DBPath string
}

func SQLiteVersion() string { return "cgo-disabled" }

func DefaultRoot() (string, error) {
	if s := os.Getenv("ROVER_HOME"); s != "" {
		return filepath.Abs(s)
	}
	p, e := os.UserConfigDir()
	if e != nil {
		return "", e
	}
	return filepath.Join(p, "rover"), nil
}

func canonicalStatePath(p string) (string, error) {
	p, e := filepath.Abs(p)
	if e != nil {
		return "", e
	}
	if s, e := os.Lstat(p); e == nil {
		if s.Mode()&os.ModeSymlink != 0 {
			return "", fmt.Errorf("state must not be a symlink: %s", p)
		}
	} else if !os.IsNotExist(e) {
		return "", e
	}
	base := p
	for {
		s, e := os.Lstat(base)
		if e == nil {
			if s.Mode()&os.ModeSymlink != 0 {
				if base == "/tmp" || base == "/var" {
					break
				}
				return "", fmt.Errorf("unsafe directory %s", base)
			}
			break
		}
		if !os.IsNotExist(e) {
			return "", e
		}
		next := filepath.Dir(base)
		if next == base {
			break
		}
		base = next
	}
	resolved, e := filepath.EvalSymlinks(base)
	if e != nil {
		return "", e
	}
	rel, e := filepath.Rel(base, p)
	if e != nil {
		return "", e
	}
	if rel == "." {
		return resolved, nil
	}
	return filepath.Join(resolved, rel), nil
}

func PrivateDir(p string) error {
	p, e := canonicalStatePath(p)
	if e != nil {
		return e
	}
	check := func() error {
		for q := p; ; q = filepath.Dir(q) {
			s, e := os.Lstat(q)
			if e == nil {
				if s.Mode()&os.ModeSymlink != 0 || !s.IsDir() {
					return fmt.Errorf("unsafe directory %s", q)
				}
			} else if !os.IsNotExist(e) {
				return e
			}
			if filepath.Dir(q) == q {
				break
			}
		}
		return nil
	}
	if e := check(); e != nil {
		return e
	}
	if e := os.MkdirAll(p, 0700); e != nil {
		return e
	}
	if e := check(); e != nil {
		return e
	}
	if f, e := os.OpenFile(p, os.O_RDONLY|syscall.O_NOFOLLOW, 0); e != nil {
		return e
	} else {
		ce := f.Chmod(0700)
		f.Close()
		return ce
	}
}

func Open(root string) (*Store, error) {
	r, e := canonicalStatePath(root)
	if e != nil {
		return nil, e
	}
	if filepath.Dir(r) == r {
		return nil, errors.New("state must be a dedicated directory, not a filesystem root")
	}
	for _, existing := range []func() (string, error){os.UserHomeDir, os.Getwd} {
		if p, err := existing(); err == nil {
			if a, err := filepath.Abs(p); err == nil && a == r {
				return nil, errors.New("state must not be the home or current working directory")
			}
		}
	}
	if entries, err := os.ReadDir(r); err == nil && len(entries) != 0 {
		st, err := os.Lstat(filepath.Join(r, "rover.db"))
		if err != nil || !st.Mode().IsRegular() {
			return nil, errors.New("nonempty state destination is not an existing Rover store; choose a dedicated empty directory")
		}
	} else if err != nil && !os.IsNotExist(err) {
		return nil, err
	}
	if e = PrivateDir(r); e != nil {
		return nil, e
	}
	for _, p := range []string{"objects", "tasks", "checks"} {
		if e := PrivateDir(filepath.Join(r, p)); e != nil {
			return nil, e
		}
	}
	return nil, errNoCGO
}

func (s *Store) Close() error { return nil }

func (s *Store) query(q string, args ...string) ([][]string, error) {
	return nil, errNoCGO
}

func (s *Store) transaction(fn func() error) error { return errNoCGO }

func (s *Store) put(kind, id string, v any, event string) error {
	if !model.ValidID(kind) || !model.ValidID(id) {
		return errors.New("invalid record identifier")
	}
	return errNoCGO
}

func (s *Store) get(kind, id string, dst any) error {
	return errNoCGO
}

func (s *Store) Put(kind, id string, v any, event string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.transaction(func() error { return s.put(kind, id, v, event) })
}

func (s *Store) Get(kind, id string, dst any) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.get(kind, id, dst)
}

func (s *Store) List(kind string, limit int) ([]json.RawMessage, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return nil, errNoCGO
}

func (s *Store) Mutate(kind, id, event string, fn func(json.RawMessage) (any, error)) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.transaction(func() error {
		return errNoCGO
	})
}

func (s *Store) CreateOnce(kind, id, key, digest string, v any) (string, bool, error) {
	return id, false, errNoCGO
}

func (s *Store) Events(id string) ([]json.RawMessage, error) {
	return nil, errNoCGO
}

func (s *Store) Blob(b []byte) (string, error) { return "", errNoCGO }

func (s *Store) ReadBlob(h string) ([]byte, error) {
	if !digestRE.MatchString(h) {
		return nil, errors.New("invalid object digest")
	}
	return nil, errNoCGO
}

func SyncDir(p string) error {
	f, e := os.Open(p)
	if e != nil {
		return e
	}
	defer f.Close()
	return f.Sync()
}

func AtomicFile(p string, b []byte, mode os.FileMode) error {
	if e := PrivateDir(filepath.Dir(p)); e != nil {
		return e
	}
	f, e := os.CreateTemp(filepath.Dir(p), ".tmp-")
	if e != nil {
		return e
	}
	tmp := f.Name()
	defer os.Remove(tmp)
	if e = f.Chmod(mode); e != nil {
		f.Close()
		return e
	}
	if _, e = f.Write(b); e != nil {
		f.Close()
		return e
	}
	if e = f.Sync(); e != nil {
		f.Close()
		return e
	}
	if e = f.Close(); e != nil {
		return e
	}
	if e = os.Rename(tmp, p); e != nil {
		return e
	}
	return SyncDir(filepath.Dir(p))
}

func (s *Store) Integrity() error { return errNoCGO }

func BoundedFile(p string, max int64) ([]byte, error) {
	f, e := os.OpenFile(p, os.O_RDONLY|syscall.O_NOFOLLOW, 0)
	if e != nil {
		return nil, e
	}
	defer f.Close()
	st, e := f.Stat()
	if e != nil {
		return nil, e
	}
	if !st.Mode().IsRegular() {
		return nil, errors.New("log is not regular")
	}
	return io.ReadAll(io.LimitReader(f, max))
}

func IsWithin(root, p string) bool {
	r, e := filepath.Rel(root, p)
	return e == nil && r != ".." && !strings.HasPrefix(r, ".."+string(filepath.Separator)) && !filepath.IsAbs(r)
}

// BackupDatabase requires SQLite. resources.go and budget.go methods
// (Acquire, Release, Delete, ListAll, BudgetUse, SetBudgetCap, Charge,
// BudgetReset, Total, String) are always compiled and call s.query/s.transaction/
// s.put/s.get, which are stubbed above.
func (s *Store) BackupDatabase(ctx context.Context, destination string) error {
	return errNoCGO
}
