package source

import (
	"context"
	"fmt"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/store"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

// Diff produces an applicable binary Git patch from retained bytes. A temporary
// bare object database avoids repository filters, hooks and moving worktrees.
func Diff(ctx context.Context, s *store.Store, base, cand model.Snapshot) ([]byte, error) {
	if e := validateSnapshot(base); e != nil {
		return nil, e
	}
	if e := validateSnapshot(cand); e != nil {
		return nil, e
	}
	dir, e := os.MkdirTemp(s.Root, "diff-")
	if e != nil {
		return nil, e
	}
	defer os.RemoveAll(dir)
	if _, e = Git(ctx, dir, "init", "--bare", "--quiet"); e != nil {
		return nil, e
	}
	cctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	cmd := exec.CommandContext(cctx, "git", "-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=false", "-C", dir, "fast-import", "--quiet")
	cmd.Env = []string{"PATH=" + os.Getenv("PATH"), "HOME=" + dir, "LC_ALL=C", "GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL=/dev/null", "GIT_NO_REPLACE_OBJECTS=1"}
	errout := &limitedBuffer{max: 32768}
	cmd.Stderr = errout
	pipe, e := cmd.StdinPipe()
	if e != nil {
		return nil, e
	}
	if e = cmd.Start(); e != nil {
		return nil, e
	}
	write := func() error {
		marks := map[string]int{}
		for _, snap := range []model.Snapshot{base, cand} {
			for _, f := range snap.Files {
				if _, ok := marks[f.SHA256]; ok {
					continue
				}
				b, e := s.ReadBlob(f.SHA256)
				if e != nil {
					return e
				}
				n := len(marks) + 1
				marks[f.SHA256] = n
				if _, e = fmt.Fprintf(pipe, "blob\nmark :%d\ndata %d\n", n, len(b)); e != nil {
					return e
				}
				if _, e = pipe.Write(b); e != nil {
					return e
				}
				if _, e = io.WriteString(pipe, "\n"); e != nil {
					return e
				}
			}
		}
		for i, snap := range []model.Snapshot{base, cand} {
			name := []string{"base", "candidate"}[i]
			if _, e := fmt.Fprintf(pipe, "commit refs/heads/%s\ncommitter Rover <rover@localhost> 1 +0000\ndata %d\n%s\n", name, len(name), name); e != nil {
				return e
			}
			for _, f := range snap.Files {
				mode := "100644"
				if f.Mode == 0755 {
					mode = "100755"
				}
				p := "\"" + strings.ReplaceAll(f.Path, "\"", "\\\"") + "\""
				if _, e := fmt.Fprintf(pipe, "M %s :%d %s\n", mode, marks[f.SHA256], p); e != nil {
					return e
				}
			}
			if _, e := io.WriteString(pipe, "\n"); e != nil {
				return e
			}
		}
		_, e := io.WriteString(pipe, "done\n")
		return e
	}
	we := write()
	pipe.Close()
	pe := cmd.Wait()
	if we != nil {
		return nil, we
	}
	if pe != nil {
		return nil, fmt.Errorf("snapshot diff import: %v %s", pe, errout.String())
	}
	return Git(ctx, filepath.Clean(dir), "diff", "--binary", "--no-ext-diff", "--no-textconv", "--no-color", "refs/heads/base", "refs/heads/candidate", "--")
}
