// Package source resolves Git inputs without checkout filters, external diffs,
// hooks, or repository commands. Snapshots contain actual file bytes in Rover's
// object store. Unsupported source states are rejected, never silently omitted.
package source

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/GrayCodeAI/rover/internal/config"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/store"
)

const MaxFile = 8 << 20
const MaxSnapshot = 128 << 20
const MaxFiles = 10000

var oidRE = regexp.MustCompile(`^[0-9a-f]{40}([0-9a-f]{24})?$`)

type limitedBuffer struct {
	bytes.Buffer
	max int
}

func (b *limitedBuffer) Write(p []byte) (int, error) {
	if len(p) > b.max-b.Len() {
		return 0, errors.New("Git output exceeded capture limit")
	}
	return b.Buffer.Write(p)
}
func gitEnv() []string {
	e := []string{}
	for _, v := range os.Environ() {
		k, _, _ := strings.Cut(v, "=")
		if strings.HasPrefix(k, "GIT_") {
			continue
		}
		e = append(e, v)
	}
	return append(e, "GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL="+os.DevNull, "GIT_TERMINAL_PROMPT=0", "GIT_NO_REPLACE_OBJECTS=1", "GIT_OPTIONAL_LOCKS=0", "LC_ALL=C")
}
func Git(ctx context.Context, repo string, args ...string) ([]byte, error) {
	ctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	fixed := []string{"--no-optional-locks", "-c", "core.hooksPath=" + os.DevNull, "-c", "core.fsmonitor=false", "-c", "core.untrackedCache=false", "-c", "core.pager=cat", "-c", "color.ui=false", "-c", "protocol.file.allow=never"}
	if repo != "" {
		fixed = append(fixed, "-C", repo)
	}
	cmd := exec.CommandContext(ctx, "git", append(fixed, args...)...)
	cmd.Env = gitEnv()
	out := &limitedBuffer{max: MaxSnapshot}
	errout := &limitedBuffer{max: 1 << 20}
	cmd.Stdout = out
	cmd.Stderr = errout
	cmd.WaitDelay = time.Second
	if e := cmd.Run(); e != nil {
		return nil, fmt.Errorf("git %s: %w: %s", args[0], e, strings.TrimSpace(errout.String()))
	}
	return out.Bytes(), nil
}
func Discover(ctx context.Context, p string) (string, error) {
	if p == "" {
		p = "."
	}
	abs, e := filepath.Abs(p)
	if e != nil {
		return "", e
	}
	b, e := Git(ctx, abs, "rev-parse", "--show-toplevel")
	if e != nil {
		return "", e
	}
	// --show-toplevel is line based; a repository root containing a newline is
	// deliberately unsupported, rather than guessed or truncated.
	root := strings.TrimSuffix(string(b), "\n")
	if !utf8.ValidString(root) || strings.ContainsAny(root, "\r\n") {
		return "", errors.New("repository root containing newline is unsupported")
	}
	return filepath.EvalSymlinks(root)
}
func Resolve(ctx context.Context, repo, ref string) (string, error) {
	if ref == "" || strings.HasPrefix(ref, "-") || strings.ContainsRune(ref, 0) || len(ref) > 1024 {
		return "", errors.New("invalid Git reference")
	}
	b, e := Git(ctx, repo, "rev-parse", "--verify", "--end-of-options", ref+"^{commit}")
	if e != nil {
		return "", e
	}
	v := strings.TrimSpace(string(b))
	if !oidRE.MatchString(v) {
		return "", errors.New("Git returned an invalid commit identity")
	}
	return v, nil
}
func safeName(p string) error {
	if !config.Relative(p) {
		return fmt.Errorf("unsafe repository path %q", p)
	}
	for _, part := range strings.Split(p, "/") {
		if strings.EqualFold(part, ".git") {
			return errors.New("nested .git paths are unsupported")
		}
	}
	return nil
}

// ReadRegular checks all path components and rejects symlinks and special files.
// Protected operation still requires an executor; same-user TOCTOU attacks are
// not prevented by these checks alone.
func ReadRegular(root, p string, max int64) ([]byte, os.FileMode, error) {
	if e := safeName(p); e != nil {
		return nil, 0, e
	}
	q := root
	for _, part := range strings.Split(p, "/") {
		q = filepath.Join(q, part)
		st, e := os.Lstat(q)
		if e != nil {
			return nil, 0, e
		}
		if st.Mode()&os.ModeSymlink != 0 {
			return nil, 0, fmt.Errorf("symlinks unsupported: %s", p)
		}
	}
	st, e := os.Lstat(q)
	if e != nil {
		return nil, 0, e
	}
	if !st.Mode().IsRegular() {
		return nil, 0, fmt.Errorf("special file or submodule unsupported: %s", p)
	}
	if st.Size() > max {
		return nil, 0, fmt.Errorf("file too large: %s", p)
	}
	f, e := os.Open(q)
	if e != nil {
		return nil, 0, e
	}
	defer f.Close()
	b, e := io.ReadAll(io.LimitReader(f, max+1))
	if e != nil {
		return nil, 0, e
	}
	if int64(len(b)) > max {
		return nil, 0, errors.New("file grew past limit")
	}
	mode := os.FileMode(0644)
	if st.Mode()&0111 != 0 {
		mode = 0755
	}
	return b, mode, nil
}
func snapshotID(repo string, files []model.File) string {
	return "snap_" + model.Hash(struct {
		Repository string
		Files      []model.File
	}{repo, files})
}
func Capture(ctx context.Context, s *store.Store, repo, ref string, includeUntracked bool) (model.Snapshot, error) {
	root, e := Discover(ctx, repo)
	if e != nil {
		return model.Snapshot{}, e
	}
	if store.IsWithin(root, s.Root) {
		return model.Snapshot{}, errors.New("Rover state must be outside the repository")
	}
	if ref == "" {
		ref = "HEAD"
	}
	type body struct {
		files  []model.File
		blobs  map[string][]byte
		commit string
	}
	collect := func() (body, error) {
		res := body{files: []model.File{}, blobs: map[string][]byte{}}
		commit, e := Resolve(ctx, root, "HEAD")
		if e != nil {
			return res, e
		}
		res.commit = commit
		total := 0
		seen := map[string]bool{}
		add := func(p string, mode uint32, b []byte) error {
			if e := safeName(p); e != nil {
				return e
			}
			if len(b) > MaxFile {
				return fmt.Errorf("file exceeds %d bytes: %s", MaxFile, p)
			}
			if bytes.HasPrefix(b, []byte("version https://git-lfs.github.com/spec/v1\n")) {
				return fmt.Errorf("unresolved Git LFS pointer: %s", p)
			}
			fold := strings.ToLower(p)
			if seen[fold] {
				return fmt.Errorf("duplicate or case-colliding source path: %s", p)
			}
			seen[fold] = true
			total += len(b)
			if total > MaxSnapshot {
				return errors.New("snapshot exceeds 128 MiB")
			}
			if len(res.files) >= MaxFiles {
				return errors.New("snapshot exceeds 10000 files")
			}
			h := model.Digest(b)
			res.files = append(res.files, model.File{Path: p, Mode: mode, SHA256: h, Size: int64(len(b))})
			res.blobs[h] = b
			return nil
		}
		if ref == "WORKTREE" {
			// Git's index exposes gitlinks even when their directory happens to be empty.
			staged, e := Git(ctx, root, "ls-files", "--stage", "-z")
			if e != nil {
				return res, e
			}
			for _, line := range bytes.Split(staged, []byte{0}) {
				if bytes.HasPrefix(line, []byte("160000 ")) {
					return res, errors.New("submodules are unsupported in this alpha")
				}
			}
			args := []string{"ls-files", "-z", "--cached"}
			if includeUntracked {
				args = append(args, "--others", "--exclude-standard")
			}
			names, e := Git(ctx, root, args...)
			if e != nil {
				return res, e
			}
			unique := map[string]bool{}
			for _, p := range strings.Split(string(names), "\x00") {
				if p == "" || unique[p] {
					continue
				}
				unique[p] = true
				// Tool output is never an implicit input. Other untracked paths require the
				// caller's include_untracked opt-in and remain subject to the size limits.
				if strings.HasPrefix(p, ".rover-results/") {
					continue
				}
				b, m, e := ReadRegular(root, p, MaxFile)
				if os.IsNotExist(e) {
					continue
				}
				if e != nil {
					return res, e
				}
				if e = add(p, uint32(m), b); e != nil {
					return res, e
				}
			}
		} else {
			commit, e = Resolve(ctx, root, ref)
			if e != nil {
				return res, e
			}
			res.commit = commit
			data, e := Git(ctx, root, "ls-tree", "-rz", "--full-tree", commit)
			if e != nil {
				return res, e
			}
			for _, line := range bytes.Split(data, []byte{0}) {
				if len(line) == 0 {
					continue
				}
				head, p, ok := strings.Cut(string(line), "\t")
				fields := strings.Fields(head)
				if !ok || len(fields) != 3 {
					return res, errors.New("malformed Git tree entry")
				}
				if fields[0] != "100644" && fields[0] != "100755" {
					return res, fmt.Errorf("unsupported Git entry mode %s at %s (symlink/submodule)", fields[0], p)
				}
				if fields[1] != "blob" || !oidRE.MatchString(fields[2]) {
					return res, errors.New("invalid Git blob")
				}
				size, e := Git(ctx, root, "cat-file", "-s", fields[2])
				if e != nil {
					return res, e
				}
				var n int64
				if _, e = fmt.Sscan(string(size), &n); e != nil || n > MaxFile || n < 0 {
					return res, fmt.Errorf("oversized or invalid Git blob: %s", p)
				}
				b, e := Git(ctx, root, "cat-file", "blob", fields[2])
				if e != nil {
					return res, e
				}
				mode := uint32(0644)
				if fields[0] == "100755" {
					mode = 0755
				}
				if e = add(p, mode, b); e != nil {
					return res, e
				}
			}
		}
		sort.Slice(res.files, func(i, j int) bool { return res.files[i].Path < res.files[j].Path })
		return res, nil
	}
	first, e := collect()
	if e != nil {
		return model.Snapshot{}, e
	}
	consistency := "exact Git commit blobs; no checkout filters"
	if ref == "WORKTREE" {
		second, e := collect()
		if e != nil {
			return model.Snapshot{}, e
		}
		if first.commit != second.commit || model.Hash(first.files) != model.Hash(second.files) {
			return model.Snapshot{}, errors.New("workspace changed during capture; pause writers and retry")
		}
		consistency = "two identical content reads; frozen bytes retained; not a filesystem-atomic capture"
	}
	snap := model.Snapshot{Schema: model.Schema, ID: snapshotID(root, first.files), Repository: root, SourceRef: ref, Commit: first.commit, Files: first.files, CreatedAt: model.Now(), Consistency: consistency}
	for _, f := range first.files {
		if _, e = s.Blob(first.blobs[f.SHA256]); e != nil {
			return model.Snapshot{}, e
		}
	}
	if e = s.Put("snapshot", snap.ID, snap, "snapshot.captured"); e != nil {
		return model.Snapshot{}, e
	}
	return snap, nil
}
func Load(s *store.Store, id string) (model.Snapshot, error) {
	var snap model.Snapshot
	if e := s.Get("snapshot", id, &snap); e != nil {
		return snap, e
	}
	if e := validateSnapshot(snap); e != nil {
		return snap, e
	}
	return snap, nil
}
func Materialize(s *store.Store, snap model.Snapshot, dest string) error {
	if e := validateSnapshot(snap); e != nil {
		return e
	}
	if e := store.PrivateDir(dest); e != nil {
		return e
	}
	for _, f := range snap.Files {
		if e := safeName(f.Path); e != nil {
			return e
		}
		if f.Mode != 0644 && f.Mode != 0755 {
			return errors.New("invalid snapshot file mode")
		}
		b, e := s.ReadBlob(f.SHA256)
		if e != nil {
			return e
		}
		if int64(len(b)) != f.Size {
			return errors.New("snapshot size mismatch")
		}
		p := filepath.Join(dest, filepath.FromSlash(f.Path))
		if e = store.PrivateDir(filepath.Dir(p)); e != nil {
			return e
		}
		out, e := os.OpenFile(p, os.O_WRONLY|os.O_CREATE|os.O_EXCL, os.FileMode(f.Mode))
		if e != nil {
			return e
		}
		_, we := out.Write(b)
		se := out.Sync()
		ce := out.Close()
		if we != nil {
			return we
		}
		if se != nil {
			return se
		}
		if ce != nil {
			return ce
		}
	}
	return nil
}
func InputsUnchanged(snap model.Snapshot, dir string) error {
	for _, f := range snap.Files {
		b, m, e := ReadRegular(dir, f.Path, MaxFile)
		if e != nil {
			return fmt.Errorf("input %s missing or unsafe: %w", f.Path, e)
		}
		if model.Digest(b) != f.SHA256 || uint32(m) != f.Mode {
			return fmt.Errorf("check modified source input %s", f.Path)
		}
	}
	return nil
}
func FileContent(s *store.Store, snap model.Snapshot, name string) ([]byte, error) {
	for _, f := range snap.Files {
		if f.Path == name {
			return s.ReadBlob(f.SHA256)
		}
	}
	return nil, store.ErrNotFound
}
func Category(p string) string {
	b := strings.ToLower(filepath.Base(p))
	p = strings.ToLower(p)
	switch {
	case strings.HasPrefix(p, ".github/") || strings.Contains(p, ".gitlab-ci"):
		return "ci"
	case strings.HasPrefix(p, ".rover/"):
		return "verification-config"
	case strings.Contains(p, "migration"):
		return "migration"
	case strings.HasPrefix(p, "docs/") || strings.HasSuffix(p, ".md"):
		return "documentation"
	case strings.Contains(b, "test") || strings.HasPrefix(p, "tests/"):
		return "test"
	case b == "go.mod" || b == "go.sum" || b == "package.json" || strings.Contains(b, "lock") || b == "requirements.txt" || b == "pyproject.toml":
		return "dependency"
	case strings.HasSuffix(b, ".yaml") || strings.HasSuffix(b, ".yml") || strings.HasSuffix(b, ".toml") || strings.HasSuffix(b, ".json"):
		return "configuration"
	default:
		return "source"
	}
}
func Compare(base, cand model.Snapshot) model.Inspection {
	old := map[string]model.File{}
	newer := map[string]model.File{}
	names := map[string]bool{}
	for _, f := range base.Files {
		old[f.Path] = f
		names[f.Path] = true
	}
	for _, f := range cand.Files {
		newer[f.Path] = f
		names[f.Path] = true
	}
	in := model.Inspection{Schema: model.Schema, Base: base, Candidate: cand, Changes: []model.Change{}, Findings: []model.Finding{}}
	sorted := []string{}
	for p := range names {
		sorted = append(sorted, p)
	}
	sort.Strings(sorted)
	for _, p := range sorted {
		a, ao := old[p]
		b, bo := newer[p]
		status := "modified"
		if !ao {
			status = "added"
		} else if !bo {
			status = "deleted"
		} else if a.SHA256 == b.SHA256 && a.Mode == b.Mode {
			continue
		}
		cat := Category(p)
		in.Changes = append(in.Changes, model.Change{Path: p, Status: status, Category: cat})
		if cat == "verification-config" || cat == "ci" {
			in.Findings = append(in.Findings, model.Finding{Rule: "verification_configuration_changed", Message: "Verification-related input changed; prior approved policy is retained", Path: p, Source: "observed_diff"})
		}
		if cat == "test" && status == "deleted" {
			in.Findings = append(in.Findings, model.Finding{Rule: "test_file_deleted", Message: "A test-classified file was deleted; classification is filename-based", Path: p, Source: "path_heuristic"})
		}
	}
	return in
}
func AddWorktree(ctx context.Context, s *store.Store, base model.Snapshot, dir string) error {
	if _, e := os.Lstat(dir); !os.IsNotExist(e) {
		return errors.New("worktree destination must not exist")
	}
	if _, e := Git(ctx, base.Repository, "worktree", "add", "--detach", "--no-checkout", dir, base.Commit); e != nil {
		return e
	}
	// No checkout is executed. Read-tree populates the index without running
	// smudge filters; source bytes come from the verified manifest.
	if _, e := Git(ctx, dir, "read-tree", base.Commit); e != nil {
		return e
	}
	return Materialize(s, base, dir)
}
