// Package store provides controller-owned SQLite records and content-addressed
// blobs. This small cgo binding uses the system SQLite library, not a custom
// database engine. All values are parameter-bound; SQL is fixed application code.
package store

/*
#cgo LDFLAGS: -lsqlite3
#include <sqlite3.h>
#include <stdlib.h>
static int bind_rover_text(sqlite3_stmt *s, int n, const char *v, int len) {
 return sqlite3_bind_text(s, n, v, len, SQLITE_TRANSIENT);
}
*/
import "C"
import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"time"
	"unsafe"

	"github.com/GrayCodeAI/rover/internal/model"
)

var ErrNotFound = errors.New("record not found")
var digestRE = regexp.MustCompile(`^[0-9a-f]{64}$`)

type Store struct {
	mu   sync.Mutex
	db   *C.sqlite3
	Root string
}

func SQLiteVersion() string { return C.GoString(C.sqlite3_libversion()) }
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
	// A symlink as the final state component is always rejected: following it
	// would operate outside the caller-named directory (see TestSymlinkStateRejected).
	if s, e := os.Lstat(p); e == nil {
		if s.Mode()&os.ModeSymlink != 0 {
			return "", fmt.Errorf("state must not be a symlink: %s", p)
		}
	} else if !os.IsNotExist(e) {
		return "", e
	}
	// Resolve OS-standard symlinked ancestors (darwin /var -> private/var,
	// /tmp -> private/tmp) by canonicalizing the nearest existing ancestor.
	// An existing ancestor that is itself a symlink (e.g. a user-planted
	// intermediate link) is rejected rather than followed.
	base := p
	for {
		s, e := os.Lstat(base)
		if e == nil {
			if s.Mode()&os.ModeSymlink != 0 {
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
	// Check existing ancestors before and after creation. This is hygiene for
	// same-user local mode, not a boundary against a malicious local administrator.
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
	return os.Chmod(p, 0700)
}
func Open(root string) (*Store, error) {
	r, e := canonicalStatePath(root)
	if e != nil {
		return nil, e
	}
	// Do not chmod or initialize a user's unrelated directory. In particular,
	// --state /, $HOME, the working directory, or a populated generic /tmp
	// directory must fail before any permissions or files are changed.
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
	dbpath := filepath.Join(r, "rover.db")
	if st, e := os.Lstat(dbpath); e == nil && !st.Mode().IsRegular() {
		return nil, errors.New("database must be a regular file")
	} else if e != nil && !os.IsNotExist(e) {
		return nil, e
	}
	s := &Store{Root: r}
	cp := C.CString(dbpath)
	defer C.free(unsafe.Pointer(cp))
	if rc := C.sqlite3_open_v2(cp, &s.db, C.SQLITE_OPEN_READWRITE|C.SQLITE_OPEN_CREATE|C.SQLITE_OPEN_FULLMUTEX, nil); rc != C.SQLITE_OK {
		e := s.dbError(rc)
		if s.db != nil {
			C.sqlite3_close(s.db)
		}
		return nil, e
	}
	if e := os.Chmod(dbpath, 0600); e != nil {
		s.Close()
		return nil, e
	}
	C.sqlite3_busy_timeout(s.db, 5000)
	for _, q := range []string{"PRAGMA journal_mode=WAL", "PRAGMA synchronous=FULL", "PRAGMA foreign_keys=ON"} {
		if _, e := s.query(q); e != nil {
			s.Close()
			return nil, e
		}
	}
	rows, e := s.query("PRAGMA user_version")
	if e != nil {
		s.Close()
		return nil, e
	}
	v, _ := strconv.Atoi(rows[0][0])
	if v > 1 {
		s.Close()
		return nil, errors.New("database schema is newer than this binary")
	}
	if v == 0 {
		e = s.transaction(func() error {
			for _, q := range []string{
				"CREATE TABLE IF NOT EXISTS records (kind TEXT NOT NULL,id TEXT NOT NULL,payload TEXT NOT NULL,updated TEXT NOT NULL,PRIMARY KEY(kind,id))",
				"CREATE TABLE IF NOT EXISTS events (seq INTEGER PRIMARY KEY AUTOINCREMENT,kind TEXT NOT NULL,ref TEXT NOT NULL,payload TEXT NOT NULL,at TEXT NOT NULL)",
				"CREATE TABLE IF NOT EXISTS request_keys (key TEXT PRIMARY KEY,digest TEXT NOT NULL,ref TEXT NOT NULL)",
				"PRAGMA user_version=1",
			} {
				if _, e := s.query(q); e != nil {
					return e
				}
			}
			return nil
		})
		if e != nil {
			s.Close()
			return nil, e
		}
	}
	return s, nil
}
func (s *Store) Close() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.db == nil {
		return nil
	}
	rc := C.sqlite3_close(s.db)
	if rc != C.SQLITE_OK {
		return s.dbError(rc)
	}
	s.db = nil
	return nil
}
func (s *Store) dbError(rc C.int) error {
	if s.db == nil {
		return fmt.Errorf("sqlite error %d", int(rc))
	}
	return fmt.Errorf("sqlite %d: %s", int(rc), C.GoString(C.sqlite3_errmsg(s.db)))
}

// query and transaction require the store mutex once the Store is published.
func (s *Store) query(q string, args ...string) ([][]string, error) {
	if s.db == nil {
		return nil, errors.New("database closed")
	}
	sql := C.CString(q)
	defer C.free(unsafe.Pointer(sql))
	var stmt *C.sqlite3_stmt
	if rc := C.sqlite3_prepare_v2(s.db, sql, -1, &stmt, nil); rc != C.SQLITE_OK {
		return nil, s.dbError(rc)
	}
	defer C.sqlite3_finalize(stmt)
	if int(C.sqlite3_bind_parameter_count(stmt)) != len(args) {
		return nil, errors.New("SQL parameter mismatch")
	}
	for i, a := range args {
		v := C.CString(a)
		rc := C.bind_rover_text(stmt, C.int(i+1), v, C.int(len(a)))
		C.free(unsafe.Pointer(v))
		if rc != C.SQLITE_OK {
			return nil, s.dbError(rc)
		}
	}
	rows := [][]string{}
	for {
		rc := C.sqlite3_step(stmt)
		if rc == C.SQLITE_DONE {
			return rows, nil
		}
		if rc != C.SQLITE_ROW {
			return nil, s.dbError(rc)
		}
		n := int(C.sqlite3_column_count(stmt))
		row := make([]string, n)
		for i := 0; i < n; i++ {
			p := C.sqlite3_column_text(stmt, C.int(i))
			l := C.sqlite3_column_bytes(stmt, C.int(i))
			if p != nil {
				row[i] = C.GoStringN((*C.char)(unsafe.Pointer(p)), l)
			}
		}
		rows = append(rows, row)
	}
}
func (s *Store) transaction(fn func() error) error {
	if _, e := s.query("BEGIN IMMEDIATE"); e != nil {
		return e
	}
	committed := false
	defer func() {
		if !committed {
			_, _ = s.query("ROLLBACK")
		}
	}()
	if e := fn(); e != nil {
		return e
	}
	if _, e := s.query("COMMIT"); e != nil {
		return e
	}
	committed = true
	return nil
}
func (s *Store) put(kind, id string, v any, event string) error {
	if !model.ValidID(kind) || !model.ValidID(id) {
		return errors.New("invalid record identifier")
	}
	b, e := json.Marshal(v)
	if e != nil {
		return e
	}
	if len(b) > 16<<20 {
		return errors.New("record exceeds 16 MiB")
	}
	if _, e = s.query("INSERT INTO records(kind,id,payload,updated) VALUES(?,?,?,?) ON CONFLICT(kind,id) DO UPDATE SET payload=excluded.payload,updated=excluded.updated", kind, id, string(b), model.Now()); e != nil {
		return e
	}
	if event != "" {
		_, e = s.query("INSERT INTO events(kind,ref,payload,at) VALUES(?,?,?,?)", event, id, string(b), model.Now())
	}
	return e
}
func (s *Store) Put(kind, id string, v any, event string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.transaction(func() error { return s.put(kind, id, v, event) })
}
func (s *Store) Get(kind, id string, dst any) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	r, e := s.query("SELECT payload FROM records WHERE kind=? AND id=?", kind, id)
	if e != nil {
		return e
	}
	if len(r) == 0 {
		return ErrNotFound
	}
	return json.Unmarshal([]byte(r[0][0]), dst)
}
func (s *Store) List(kind string, limit int) ([]json.RawMessage, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if limit < 1 || limit > 1000 {
		limit = 100
	}
	r, e := s.query("SELECT payload FROM records WHERE kind=? ORDER BY updated DESC,id DESC LIMIT ?", kind, strconv.Itoa(limit))
	if e != nil {
		return nil, e
	}
	v := make([]json.RawMessage, 0, len(r))
	for _, x := range r {
		v = append(v, json.RawMessage(x[0]))
	}
	return v, nil
}
func (s *Store) Mutate(kind, id, event string, fn func(json.RawMessage) (any, error)) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.transaction(func() error {
		r, e := s.query("SELECT payload FROM records WHERE kind=? AND id=?", kind, id)
		if e != nil {
			return e
		}
		if len(r) == 0 {
			return ErrNotFound
		}
		v, e := fn(json.RawMessage(r[0][0]))
		if e != nil {
			return e
		}
		return s.put(kind, id, v, event)
	})
}

// CreateOnce atomically commits an idempotency key, initial record and event.
// It never launches a process and never retries external effects.
func (s *Store) CreateOnce(kind, id, key, digest string, v any) (string, bool, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	actual := id
	exists := false
	e := s.transaction(func() error {
		if key != "" {
			r, e := s.query("SELECT digest,ref FROM request_keys WHERE key=?", key)
			if e != nil {
				return e
			}
			if len(r) > 0 {
				if r[0][0] != digest {
					return errors.New("idempotency key reused with a different contract")
				}
				actual = r[0][1]
				exists = true
				return nil
			}
		}
		if e := s.put(kind, id, v, kind+".created"); e != nil {
			return e
		}
		if key != "" {
			_, e := s.query("INSERT INTO request_keys(key,digest,ref) VALUES(?,?,?)", key, digest, id)
			return e
		}
		return nil
	})
	return actual, exists, e
}
func (s *Store) Events(id string) ([]json.RawMessage, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	r, e := s.query("SELECT seq,kind,ref,at FROM events WHERE ref=? ORDER BY seq LIMIT 1000", id)
	if e != nil {
		return nil, e
	}
	v := []json.RawMessage{}
	for _, x := range r {
		b, _ := json.Marshal(map[string]string{"sequence": x[0], "event": x[1], "ref": x[2], "at": x[3]})
		v = append(v, b)
	}
	return v, nil
}
func (s *Store) Blob(b []byte) (string, error) {
	h := model.Digest(b)
	dir := filepath.Join(s.Root, "objects")
	p := filepath.Join(dir, h)
	f, e := os.CreateTemp(dir, ".object-")
	if e != nil {
		return "", e
	}
	tmp := f.Name()
	defer os.Remove(tmp)
	if e = f.Chmod(0600); e != nil {
		f.Close()
		return "", e
	}
	if _, e = f.Write(b); e != nil {
		f.Close()
		return "", e
	}
	if e = f.Sync(); e != nil {
		f.Close()
		return "", e
	}
	if e = f.Close(); e != nil {
		return "", e
	}
	if e = os.Link(tmp, p); e != nil {
		if !os.IsExist(e) {
			return "", e
		}
		if _, e = s.ReadBlob(h); e != nil {
			return "", e
		}
	}
	if e = SyncDir(dir); e != nil {
		return "", e
	}
	return h, nil
}
func (s *Store) ReadBlob(h string) ([]byte, error) {
	if !digestRE.MatchString(h) {
		return nil, errors.New("invalid object digest")
	}
	p := filepath.Join(s.Root, "objects", h)
	st, e := os.Lstat(p)
	if e != nil {
		return nil, e
	}
	if !st.Mode().IsRegular() {
		return nil, errors.New("object is not regular")
	}
	if st.Size() > 16<<20 {
		return nil, errors.New("object too large")
	}
	b, e := os.ReadFile(p)
	if e != nil {
		return nil, e
	}
	if model.Digest(b) != h {
		return nil, errors.New("object digest mismatch")
	}
	return b, nil
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
func (s *Store) Integrity() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	r, e := s.query("PRAGMA quick_check")
	if e != nil {
		return e
	}
	if len(r) != 1 || r[0][0] != "ok" {
		return fmt.Errorf("database integrity check failed: %v", r)
	}
	return nil
}

// BoundedFile is used for human-facing logs, never as a substitute for trusted
// evidence. Root is checked separately by the caller.
func BoundedFile(p string, max int64) ([]byte, error) {
	st, e := os.Lstat(p)
	if e != nil {
		return nil, e
	}
	if !st.Mode().IsRegular() {
		return nil, errors.New("log is not regular")
	}
	f, e := os.Open(p)
	if e != nil {
		return nil, e
	}
	defer f.Close()
	return io.ReadAll(io.LimitReader(f, max))
}
func IsWithin(root, p string) bool {
	r, e := filepath.Rel(root, p)
	return e == nil && r != ".." && !strings.HasPrefix(r, ".."+string(filepath.Separator)) && !filepath.IsAbs(r)
}

// BackupDatabase uses SQLite's online backup API, never a raw copy of an active
// WAL file. The destination must not exist. Context cancellation bounds retries.
func (s *Store) BackupDatabase(ctx context.Context, destination string) (err error) {
	f, e := os.OpenFile(destination, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if e != nil {
		return e
	}
	if e = f.Close(); e != nil {
		return e
	}
	success := false
	defer func() {
		if !success {
			_ = os.Remove(destination)
		}
	}()
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.db == nil {
		return errors.New("database closed")
	}
	path := C.CString(destination)
	defer C.free(unsafe.Pointer(path))
	var target *C.sqlite3
	if rc := C.sqlite3_open_v2(path, &target, C.SQLITE_OPEN_READWRITE|C.SQLITE_OPEN_FULLMUTEX, nil); rc != C.SQLITE_OK {
		if target != nil {
			C.sqlite3_close(target)
		}
		return fmt.Errorf("backup destination sqlite error %d", int(rc))
	}
	defer func() {
		rc := C.sqlite3_close(target)
		if err == nil && rc != C.SQLITE_OK {
			err = fmt.Errorf("backup close failed: %d", int(rc))
		}
	}()
	main := C.CString("main")
	defer C.free(unsafe.Pointer(main))
	b := C.sqlite3_backup_init(target, main, s.db, main)
	if b == nil {
		return errors.New("SQLite backup initialization failed")
	}
	finished := false
	defer func() {
		if !finished {
			C.sqlite3_backup_finish(b)
		}
	}()
	for {
		if e = ctx.Err(); e != nil {
			return e
		}
		rc := C.sqlite3_backup_step(b, 128)
		if rc == C.SQLITE_DONE {
			break
		}
		if rc != C.SQLITE_OK && rc != C.SQLITE_BUSY && rc != C.SQLITE_LOCKED {
			return fmt.Errorf("SQLite backup step failed: %d", int(rc))
		}
		if rc == C.SQLITE_BUSY || rc == C.SQLITE_LOCKED {
			select {
			case <-ctx.Done():
				return ctx.Err()
			case <-time.After(10 * time.Millisecond):
			}
		}
	}
	rc := C.sqlite3_backup_finish(b)
	finished = true
	if rc != C.SQLITE_OK {
		return fmt.Errorf("SQLite backup finish failed: %d", int(rc))
	}
	success = true
	return nil
}
