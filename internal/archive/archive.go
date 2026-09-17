// Package archive backs up controller metadata and retained immutable objects.
// It never promises to preserve live process state or uncommitted agent work.
package archive

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"syscall"

	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/store"
	"github.com/GrayCodeAI/rover/internal/wire"
)

const MaxBackupBytes int64 = 1 << 30
const MaxFiles = 10000

type File struct {
	Path   string `json:"path"`
	SHA256 string `json:"sha256"`
	Size   int64  `json:"size"`
}
type Manifest struct {
	Schema       string `json:"schema"`
	RoverVersion string `json:"rover_version"`
	CreatedAt    string `json:"created_at"`
	Files        []File `json:"files"`
	Scope        string `json:"scope"`
}

func resolveBackupDir(path string) (string, error) {
	a, e := filepath.Abs(path)
	if e != nil {
		return "", e
	}
	// Resolve OS-standard symlinked ancestors (darwin /var -> private/var,
	// /tmp -> private/tmp) via the nearest existing ancestor. An existing
	// ancestor that is itself a symlink is rejected rather than followed, so
	// a planted intermediate link cannot redirect backup/restore output.
	base := filepath.Dir(a)
	for {
		s, e := os.Lstat(base)
		if e == nil {
			if s.Mode()&os.ModeSymlink != 0 {
				if base == "/tmp" || base == "/var" {
					break
				}
				return "", errors.New("symlink backup parent unsupported")
			}
			break
		}
		if !os.IsNotExist(e) {
			return "", e
		}
		next := filepath.Dir(base)
		if next == base {
			return "", errors.New("symlink backup parent unsupported")
		}
		base = next
	}
	resolved, e := filepath.EvalSymlinks(base)
	if e != nil {
		return "", e
	}
	rel, e := filepath.Rel(base, a)
	if e != nil {
		return "", e
	}
	if rel == "." {
		return resolved, nil
	}
	return filepath.Join(resolved, rel), nil
}
func newDir(path string) error {
	a, e := resolveBackupDir(path)
	if e != nil {
		return e
	}
	return os.Mkdir(a, 0700)
}
func readFile(root, name string, max int64) ([]byte, error) {
	if name != "rover.db" && name != "manifest.json" && !(strings.HasPrefix(name, "objects/") && len(strings.TrimPrefix(name, "objects/")) == 64 && strings.Trim(strings.TrimPrefix(name, "objects/"), "0123456789abcdef") == "") {
		return nil, errors.New("unsupported archive path")
	}
	p := filepath.Join(root, filepath.FromSlash(name))
	for q := p; ; q = filepath.Dir(q) {
		st, e := os.Lstat(q)
		if e != nil {
			return nil, e
		}
		if st.Mode()&os.ModeSymlink != 0 {
			return nil, errors.New("symlink in archive")
		}
		if q == root {
			break
		}
		if !store.IsWithin(root, q) {
			return nil, errors.New("archive path escape")
		}
	}
	st, e := os.Stat(p)
	if e != nil {
		return nil, e
	}
	if !st.Mode().IsRegular() || st.Size() < 0 || st.Size() > max {
		return nil, errors.New("archive file oversized or nonregular")
	}
	f, e := os.OpenFile(p, os.O_RDONLY|syscall.O_NOFOLLOW, 0)
	if e != nil {
		return nil, e
	}
	defer f.Close()
	if st, e := f.Stat(); e != nil || !st.Mode().IsRegular() {
		return nil, errors.New("archive file changed during open")
	}
	b, e := io.ReadAll(io.LimitReader(f, max+1))
	if e != nil {
		return nil, e
	}
	if int64(len(b)) > max {
		return nil, errors.New("archive file grew beyond bound")
	}
	return b, nil
}
func resolveExistingDir(path string) (string, error) {
	a, e := filepath.Abs(path)
	if e != nil {
		return "", e
	}
	if resolved, e := filepath.EvalSymlinks(a); e == nil {
		return resolved, nil
	}
	return a, nil
}
func Backup(ctx context.Context, s *store.Store, destination string) (Manifest, error) {
	m := Manifest{Schema: "rover.backup/v1", RoverVersion: model.Version, CreatedAt: model.Now(), Scope: "SQLite point-in-time metadata and retained objects; excludes live sessions, uncaptured workspaces and raw task logs; sensitive content may exist"}
	dest, e := resolveBackupDir(destination)
	if e != nil {
		return m, e
	}
	if store.IsWithin(s.Root, dest) {
		return m, errors.New("backup must be outside state")
	}
	if e = newDir(dest); e != nil {
		return m, e
	}
	// An incomplete backup intentionally lacks a valid manifest and is never
	// accepted by restore. Do not delete arbitrary user paths on failure.
	if e = s.BackupDatabase(ctx, filepath.Join(dest, "rover.db")); e != nil {
		return m, e
	}
	if e = os.Mkdir(filepath.Join(dest, "objects"), 0700); e != nil {
		return m, e
	}
	db, e := readFile(dest, "rover.db", MaxBackupBytes)
	if e != nil {
		return m, e
	}
	m.Files = append(m.Files, File{"rover.db", model.Digest(db), int64(len(db))})
	total := int64(len(db))
	entries, e := os.ReadDir(filepath.Join(s.Root, "objects"))
	if e != nil {
		return m, e
	}
	for _, x := range entries {
		if e = ctx.Err(); e != nil {
			return m, e
		}
		if strings.HasPrefix(x.Name(), ".object-") {
			continue
		}
		if len(m.Files) >= MaxFiles {
			return m, errors.New("backup file limit reached")
		}
		b, e := s.ReadBlob(x.Name())
		if e != nil {
			return m, e
		}
		total += int64(len(b))
		if total > MaxBackupBytes {
			return m, errors.New("backup byte budget exceeded")
		}
		name := "objects/" + x.Name()
		objPath := filepath.Join(dest, filepath.FromSlash(name))
		if f, e := os.OpenFile(objPath, os.O_WRONLY|os.O_CREATE|os.O_EXCL|syscall.O_NOFOLLOW, 0600); e != nil {
			return m, e
		} else if _, e = f.Write(b); e != nil {
			f.Close()
			return m, e
		} else if e = f.Close(); e != nil {
			return m, e
		}
		m.Files = append(m.Files, File{name, model.Digest(b), int64(len(b))})
	}
	sort.Slice(m.Files, func(i, j int) bool { return m.Files[i].Path < m.Files[j].Path })
	b, e := json.MarshalIndent(m, "", "  ")
	if e != nil {
		return m, e
	}
	if len(b) > 1<<20 {
		return m, errors.New("manifest exceeds format bound")
	}
	if e = store.AtomicFile(filepath.Join(dest, "manifest.json"), b, 0600); e != nil {
		return m, e
	}
	return m, store.SyncDir(dest)
}
func Verify(directory string) (Manifest, error) {
	var m Manifest
	root, e := resolveExistingDir(directory)
	if e != nil {
		return m, e
	}
	b, e := readFile(root, "manifest.json", 1<<20)
	if e != nil {
		return m, e
	}
	if e = wire.Decode(b, &m); e != nil {
		return m, e
	}
	if m.Schema != "rover.backup/v1" || len(m.Files) == 0 || len(m.Files) > MaxFiles {
		return m, errors.New("invalid backup manifest")
	}
	seen := map[string]bool{}
	total := int64(0)
	for _, f := range m.Files {
		if seen[f.Path] || f.Path == "manifest.json" || f.Size < 0 {
			return m, errors.New("duplicate or invalid archive entry")
		}
		seen[f.Path] = true
		total += f.Size
		if total > MaxBackupBytes {
			return m, errors.New("backup byte limit exceeded")
		}
		b, e := readFile(root, f.Path, f.Size)
		if e != nil {
			return m, e
		}
		if int64(len(b)) != f.Size || model.Digest(b) != f.SHA256 {
			return m, fmt.Errorf("archive digest mismatch: %s", f.Path)
		}
		if strings.HasPrefix(f.Path, "objects/") && strings.TrimPrefix(f.Path, "objects/") != f.SHA256 {
			return m, errors.New("object identity mismatch")
		}
	}
	if !seen["rover.db"] {
		return m, errors.New("metadata database missing")
	}
	return m, nil
}
func Restore(ctx context.Context, directory, destination string) (Manifest, error) {
	m, e := Verify(directory)
	if e != nil {
		return m, e
	}
	src, e := resolveExistingDir(directory)
	if e != nil {
		return m, e
	}
	dest, e := resolveBackupDir(destination)
	if e != nil {
		return m, e
	}
	if store.IsWithin(src, dest) || store.IsWithin(dest, src) {
		return m, errors.New("restore and backup must be separate trees")
	}
	if e = newDir(dest); e != nil {
		return m, e
	}
	if e = os.Mkdir(filepath.Join(dest, "objects"), 0700); e != nil {
		return m, e
	}
	for _, f := range m.Files {
		if e = ctx.Err(); e != nil {
			return m, e
		}
		b, e := readFile(src, f.Path, f.Size)
		if e != nil {
			return m, e
		}
		if model.Digest(b) != f.SHA256 {
			return m, errors.New("backup changed during restore")
		}
		destPath := filepath.Join(dest, filepath.FromSlash(f.Path))
		if !store.IsWithin(dest, destPath) {
			return m, errors.New("restore path escape")
		}
		if tmp, e := os.OpenFile(destPath, os.O_WRONLY|os.O_CREATE|os.O_EXCL|syscall.O_NOFOLLOW, 0600); e != nil {
			return m, e
		} else if _, e = tmp.Write(b); e != nil {
			tmp.Close()
			return m, e
		} else if e = tmp.Sync(); e != nil {
			tmp.Close()
			return m, e
		} else {
			tmp.Close()
		}
	}
	s, e := store.Open(dest)
	if e != nil {
		return m, e
	}
	defer s.Close()
	if e = s.Integrity(); e != nil {
		return m, e
	}
	for _, kind := range []string{"task", "workflow", "grant", "lease"} {
		rows, e := s.ListAll(kind, 100000)
		if e != nil {
			return m, e
		}
		for _, raw := range rows {
			var obj map[string]json.RawMessage
			if e = json.Unmarshal(raw, &obj); e != nil {
				return m, e
			}
			var id string
			_ = json.Unmarshal(obj["id"], &id)
			if kind == "lease" {
				var l store.Lease
				if e = json.Unmarshal(raw, &l); e != nil {
					return m, e
				}
				if e = s.Delete(kind, l.Resource, "lease.restore_released"); e != nil {
					return m, e
				}
				continue
			}
			if id == "" {
				return m, errors.New("restored record missing ID")
			}
			if kind == "grant" {
				obj["revoked"] = json.RawMessage("true")
			} else {
				var status string
				_ = json.Unmarshal(obj["status"], &status)
				if !model.Terminal(status) && status != "CONFLICT" {
					obj["status"] = json.RawMessage(`"LOST"`)
					obj["error"] = json.RawMessage(`"restored evidence backup; process state not restored; no automatic execution"`)
				}
				obj["pid"] = json.RawMessage("0")
				delete(obj, "socket")
				delete(obj, "process_identity")
			}
			if e = s.Put(kind, id, obj, "record.restore_sanitized"); e != nil {
				return m, e
			}
		}
	}
	return m, nil
}
