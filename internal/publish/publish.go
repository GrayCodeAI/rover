package publish

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"syscall"

	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/store"
)

// Publish writes a local, advisory copy of an investigation to destDir.
// It is not a protected CI publisher — it is a local file export that can
// be inspected, not an independent acceptance gate.
func Publish(s *store.Store, investigationID, destDir string) (string, error) {
	if !model.ValidID(investigationID) {
		return "", errors.New("invalid investigation ID")
	}
	var inv model.Investigation
	if e := s.Get("investigation", investigationID, &inv); e != nil {
		return "", e
	}
	if e := os.MkdirAll(destDir, 0700); e != nil {
		return "", e
	}
	if st, e := os.Lstat(destDir); e != nil || st.Mode()&os.ModeSymlink != 0 || !st.IsDir() {
		return "", errors.New("publish destination is not a directory")
	}
	// Ensure dest is not inside state (avoid recursion) and is a directory.
	if store.IsWithin(s.Root, destDir) {
		return "", errors.New("publish destination must be outside state")
	}
	data, e := json.MarshalIndent(inv, "", "  ")
	if e != nil {
		return "", e
	}
	if len(data) > 16<<20 {
		return "", errors.New("investigation export exceeds bound")
	}
	out := filepath.Join(destDir, investigationID+".json")
	if !store.IsWithin(destDir, out) {
		return "", errors.New("publish path escape")
	}
	f, e := os.OpenFile(out, os.O_WRONLY|os.O_CREATE|os.O_EXCL|syscall.O_NOFOLLOW, 0600)
	if e != nil {
		return "", e
	}
	if _, e = f.Write(data); e != nil {
		f.Close()
		return "", e
	}
	if e = f.Close(); e != nil {
		return "", e
	}
	return out, nil
}
