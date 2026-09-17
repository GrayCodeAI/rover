package source

import (
	"errors"
	"fmt"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/store"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

func validateSnapshot(snap model.Snapshot) error {
	if snap.Schema != model.Schema || snap.ID != snapshotID(snap.Repository, snap.Files) {
		return errors.New("snapshot identity mismatch")
	}
	if !oidRE.MatchString(snap.Commit) {
		return errors.New("invalid snapshot commit identity")
	}
	if len(snap.Files) > MaxFiles {
		return errors.New("snapshot has too many files")
	}
	total := int64(0)
	seen := map[string]bool{}
	for _, f := range snap.Files {
		if e := safeName(f.Path); e != nil {
			return e
		}
		fold := strings.ToLower(f.Path)
		if seen[fold] {
			return errors.New("duplicate/case-colliding path")
		}
		seen[fold] = true
		if f.Mode != 0644 && f.Mode != 0755 {
			return errors.New("invalid file mode")
		}
		if f.Size < 0 || f.Size > MaxFile {
			return errors.New("invalid file size")
		}
		total += f.Size
		if len(f.SHA256) != 64 || strings.Trim(f.SHA256, "0123456789abcdef") != "" {
			return errors.New("invalid object hash")
		}
	}
	if total > MaxSnapshot {
		return errors.New("snapshot size budget exceeded")
	}
	for p := range seen {
		for q := filepath.ToSlash(filepath.Dir(p)); q != "."; q = filepath.ToSlash(filepath.Dir(q)) {
			if seen[q] {
				return errors.New("file/directory conflict")
			}
		}
	}
	return nil
}

// Compose materializes identities from already retained objects, without running
// a checkout filter, hook, or repository program.
func Compose(s *store.Store, origin model.Snapshot, files []model.File, label string) (model.Snapshot, error) {
	files = append([]model.File(nil), files...)
	sort.Slice(files, func(i, j int) bool { return files[i].Path < files[j].Path })
	out := origin
	out.Files = files
	out.SourceRef = label
	out.CreatedAt = model.Now()
	out.Consistency = "composed from verified content objects"
	out.ID = snapshotID(out.Repository, files)
	if e := validateSnapshot(out); e != nil {
		return out, e
	}
	for _, f := range files {
		b, e := s.ReadBlob(f.SHA256)
		if e != nil {
			return out, e
		}
		if int64(len(b)) != f.Size || model.Digest(b) != f.SHA256 {
			return out, errors.New("object size mismatch")
		}
	}
	return out, s.Put("snapshot", out.ID, out, "snapshot.composed")
}
func Overlay(s *store.Store, base, cand model.Snapshot, paths []string) (model.Snapshot, error) {
	if len(paths) == 0 {
		return base, errors.New("explicit overlay paths required")
	}
	old := map[string]model.File{}
	next := map[string]model.File{}
	for _, f := range base.Files {
		old[f.Path] = f
	}
	for _, f := range cand.Files {
		next[f.Path] = f
	}
	seen := map[string]bool{}
	for _, p := range paths {
		if e := safeName(p); e != nil {
			return base, e
		}
		if seen[p] {
			return base, errors.New("duplicate overlay")
		}
		seen[p] = true
		f, ok := next[p]
		if !ok {
			return base, fmt.Errorf("overlay file absent: %s", p)
		}
		old[p] = f
	}
	fs := []model.File{}
	for _, f := range old {
		fs = append(fs, f)
	}
	return Compose(s, base, fs, "overlay:"+cand.ID)
}

// Merge is intentionally conservative: file-disjoint changes and identical
// edits compose; contradictory edits require an explicit integration task.
func Merge(s *store.Store, base model.Snapshot, candidates []model.Snapshot) (model.Snapshot, error) {
	result := map[string]model.File{}
	for _, f := range base.Files {
		result[f.Path] = f
	}
	edits := map[string]model.File{}
	dels := map[string]bool{}
	for _, c := range candidates {
		for _, ch := range Compare(base, c).Changes {
			var n model.File
			for _, f := range c.Files {
				if f.Path == ch.Path {
					n = f
					break
				}
			}
			old, had := edits[ch.Path]
			del := ch.Status == "deleted"
			if had && (dels[ch.Path] != del || (!del && old != n)) {
				return model.Snapshot{}, fmt.Errorf("integration conflict: %s", ch.Path)
			}
			edits[ch.Path] = n
			dels[ch.Path] = del
			if del {
				delete(result, ch.Path)
			} else {
				result[ch.Path] = n
			}
		}
	}
	fs := []model.File{}
	for _, f := range result {
		fs = append(fs, f)
	}
	return Compose(s, base, fs, "integration")
}
func ReplaceWorkspace(s *store.Store, from, to model.Snapshot, dir string) error {
	if !store.IsWithin(filepath.Join(s.Root, "tasks"), dir) {
		return errors.New("replacement outside Rover task storage")
	}
	if e := InputsUnchanged(from, dir); e != nil {
		return e
	}
	for _, f := range from.Files {
		if e := os.Remove(filepath.Join(dir, filepath.FromSlash(f.Path))); e != nil {
			return e
		}
	}
	return Materialize(s, to, dir)
}
